//! Passwords and API keys are kept in the system's keyring, never written to `config.json` by the island: the Secret
//! Service on Linux (GNOME Keyring, KWallet, KeePassXC), the Credential Manager on Windows, the Keychain on macOS.
//! One written into the file by hand is moved there (see `config::migrate_secrets`). Any program running as the same
//! user can ask an unlocked keyring for them: what this protects is the file, its copies and its backups.

use island_ui::model::{Command, OptionValue};
use std::collections::HashMap;
use std::sync::Mutex;

/// The account the API key of the map tiles is kept under.
pub const MAP_KEY: &str = "map-api-key";

/// The account the password of a CalDAV sign-in is kept under.
pub fn caldav_account(server: &str, user: &str) -> String {
    format!("caldav:{user}@{server}")
}

// tests never touch the keyring of the person who runs them
#[cfg(test)]
use memory as backend;
#[cfg(all(not(test), any(target_os = "linux", windows, target_os = "macos")))]
use os as backend;
#[cfg(all(not(test), not(any(target_os = "linux", windows, target_os = "macos"))))]
use none as backend;

/// One keyring operation at a time: several threads asking a locked keyring would mean several unlock windows.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// What was read for the callers that ask very often (a map asks for its key at every tile).
static CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn locked<T>(f: impl FnOnce() -> T) -> T {
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

fn forget_cached(account: &str) {
    if let Some(c) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        c.remove(account);
    }
}

/// The secret of an account; None when the keyring holds none.
pub fn get(account: &str) -> Result<Option<String>, String> {
    locked(|| backend::read(account))
}

/// `get`, remembered until the secret is changed through the island.
pub fn cached(account: &str) -> Option<String> {
    if let Some(s) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(account)) {
        return Some(s.clone());
    }
    match get(account) {
        Ok(Some(s)) => {
            CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(account.to_string(), s.clone());
            Some(s)
        }
        Ok(None) => None,
        Err(e) => {
            log::debug!("keyring: {e}");
            None
        }
    }
}

/// Stores a secret and reads it back: Ok only when the keyring really holds it.
pub fn set(account: &str, secret: &str) -> Result<(), String> {
    forget_cached(account);
    locked(|| {
        backend::write(account, secret)?;
        match backend::read(account)? {
            Some(s) if s == secret => Ok(()),
            _ => Err("the keyring did not keep what it was given".to_string()),
        }
    })
}

pub fn delete(account: &str) -> Result<(), String> {
    forget_cached(account);
    locked(|| backend::erase(account))
}

/// Whether there is a keyring to talk to (for `doctor`).
pub fn probe() -> Result<(), String> {
    locked(backend::probe)
}

/// The settings commands that carry a secret: they are handled here, off the interface thread, because a locked keyring
/// waits for the person to unlock it.
pub fn handles(c: &Command) -> bool {
    match c {
        Command::AddCalDav { .. } | Command::AddICloud { .. } => true,
        Command::SetOption { key, .. } => key == "map_api_key",
        _ => false,
    }
}

pub fn command(rep: &super::Reporter, c: Command) {
    let rep = rep.clone();
    std::thread::spawn(move || {
        let result = crate::config::path().ok_or_else(|| "no configuration directory".to_string()).and_then(|p| apply(&p, &c));
        rep.config_saved(result);
    });
}

/// Puts the secret of a command in the keyring, then writes the rest into the settings file. When the keyring refuses,
/// the file is left as it was.
fn apply(path: &std::path::Path, c: &Command) -> Result<(), String> {
    use crate::config;
    match c {
        Command::AddCalDav { server, user, password } => {
            set(&caldav_account(server, user), password)?;
            config::update_calendars_at(path, |list| config::add_caldav(list, server, user))
        }
        Command::AddICloud { user, password } => {
            set(&caldav_account(config::ICLOUD_SERVER, user), password)?;
            config::update_calendars_at(path, |list| config::add_icloud(list, user))
        }
        Command::SetOption { key, value: OptionValue::Text(t) } if key == "map_api_key" => {
            let t = config::check_api_key(t)?;
            if t.is_empty() {
                let stored = std::fs::read_to_string(path).ok().and_then(|f| config::parse(&f).ok()).is_some_and(|s| s.map_key_in_keyring);
                if stored {
                    delete(MAP_KEY)?;
                }
            } else {
                set(MAP_KEY, t)?;
            }
            config::update_at(path, |o| config::mark_map_key(o, !t.is_empty()))
        }
        _ => Err("an API key is text".to_string()),
    }
}

/// The Secret Service of freedesktop.org, over the session bus. The "plain" session sends the secret as it is on that
/// bus, which only this user's programs can reach.
#[cfg(target_os = "linux")]
#[cfg_attr(test, allow(dead_code))]
mod os {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

    const BUS: &str = "org.freedesktop.secrets";
    const ROOT: &str = "/org/freedesktop/secrets";

    /// The person closed the unlock window: reading does not open it again until a secret is saved.
    static DISMISSED: AtomicBool = AtomicBool::new(false);

    fn explain(e: zbus::Error) -> String {
        let e = e.to_string();
        if e.contains("ServiceUnknown") || e.contains("NameHasNoOwner") || e.contains("not provided by any") {
            "no keyring is running (GNOME Keyring, KWallet or KeePassXC)".to_string()
        } else {
            format!("keyring: {}", e.chars().take(80).collect::<String>())
        }
    }

    fn proxy<'a>(conn: &Connection, path: &'a str, interface: &'static str) -> Result<Proxy<'a>, String> {
        Proxy::new(conn, BUS, path, interface).map_err(explain)
    }

    fn service(conn: &Connection) -> Result<Proxy<'static>, String> {
        proxy(conn, ROOT, "org.freedesktop.Secret.Service")
    }

    fn attributes(account: &str) -> HashMap<&str, &str> {
        HashMap::from([("application", "open-island"), ("account", account)])
    }

    /// Shows the keyring's own window (to unlock it, or to confirm) and waits for the person.
    fn prompt(conn: &Connection, path: &OwnedObjectPath) -> Result<(), String> {
        if path.as_str() == "/" {
            return Ok(());
        }
        let p = proxy(conn, path.as_str(), "org.freedesktop.Secret.Prompt")?;
        let mut done = p.receive_signal("Completed").map_err(explain)?;
        p.call_method("Prompt", &("",)).map_err(explain)?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let dismissed = done.next().and_then(|m| m.body().deserialize::<(bool, OwnedValue)>().ok()).is_none_or(|(d, _)| d);
                let _ = tx.send(dismissed);
            });
            let answer = rx.recv_timeout(Duration::from_secs(90));
            if answer.is_err() {
                // closing the prompt makes it send Completed, which ends the waiting thread
                let _ = p.call_method("Dismiss", &());
            }
            match answer {
                Ok(false) => Ok(()),
                _ => {
                    DISMISSED.store(true, Ordering::Relaxed);
                    Err("the keyring was not unlocked".to_string())
                }
            }
        })
    }

    fn unlock(conn: &Connection, object: &OwnedObjectPath) -> Result<(), String> {
        let (_, ask): (Vec<OwnedObjectPath>, OwnedObjectPath) = service(conn)?.call("Unlock", &(vec![object],)).map_err(explain)?;
        if ask.as_str() != "/" && DISMISSED.load(Ordering::Relaxed) {
            return Err("the keyring is locked".to_string());
        }
        prompt(conn, &ask)
    }

    fn default_collection(conn: &Connection) -> Result<OwnedObjectPath, String> {
        let path: OwnedObjectPath = service(conn)?.call("ReadAlias", &("default",)).map_err(explain)?;
        if path.as_str() == "/" {
            return Err("the keyring has no default collection: create one in its own program".to_string());
        }
        Ok(path)
    }

    fn open_session(conn: &Connection) -> Result<OwnedObjectPath, String> {
        let (_, session): (OwnedValue, OwnedObjectPath) = service(conn)?.call("OpenSession", &("plain", Value::from(""))).map_err(explain)?;
        Ok(session)
    }

    /// The items of an account, and whether they are behind a lock.
    fn find(conn: &Connection, account: &str) -> Result<(Vec<OwnedObjectPath>, Vec<OwnedObjectPath>), String> {
        service(conn)?.call("SearchItems", &(attributes(account),)).map_err(explain)
    }

    pub fn probe() -> Result<(), String> {
        let conn = Connection::session().map_err(explain)?;
        default_collection(&conn).map(|_| ())
    }

    pub fn read(account: &str) -> Result<Option<String>, String> {
        let conn = Connection::session().map_err(explain)?;
        let (open, shut) = find(&conn, account)?;
        let item = match (open.into_iter().next(), shut.into_iter().next()) {
            (Some(i), _) => i,
            (None, Some(i)) => {
                unlock(&conn, &i)?;
                i
            }
            (None, None) => return Ok(None),
        };
        let session = open_session(&conn)?;
        let (_, _, value, _): (OwnedObjectPath, Vec<u8>, Vec<u8>, String) = proxy(&conn, item.as_str(), "org.freedesktop.Secret.Item")?.call("GetSecret", &(&session,)).map_err(explain)?;
        String::from_utf8(value).map(Some).map_err(|_| "keyring: the secret is not text".to_string())
    }

    pub fn write(account: &str, secret: &str) -> Result<(), String> {
        DISMISSED.store(false, Ordering::Relaxed);
        let conn = Connection::session().map_err(explain)?;
        let collection = default_collection(&conn)?;
        unlock(&conn, &collection)?;
        let session = open_session(&conn)?;
        let properties: HashMap<&str, Value> = HashMap::from([
            ("org.freedesktop.Secret.Item.Label", Value::from(format!("open-island: {account}"))),
            ("org.freedesktop.Secret.Item.Attributes", Value::from(attributes(account))),
        ]);
        let value: (&ObjectPath, Vec<u8>, &[u8], &str) = (&session, vec![], secret.as_bytes(), "text/plain; charset=utf8");
        // true: an item with the same attributes is replaced
        let (_, ask): (OwnedObjectPath, OwnedObjectPath) = proxy(&conn, collection.as_str(), "org.freedesktop.Secret.Collection")?.call("CreateItem", &(properties, value, true)).map_err(explain)?;
        prompt(&conn, &ask)
    }

    pub fn erase(account: &str) -> Result<(), String> {
        DISMISSED.store(false, Ordering::Relaxed);
        let conn = Connection::session().map_err(explain)?;
        let (open, shut) = find(&conn, account)?;
        for item in open.iter().chain(&shut) {
            let ask: OwnedObjectPath = proxy(&conn, item.as_str(), "org.freedesktop.Secret.Item")?.call("Delete", &()).map_err(explain)?;
            prompt(&conn, &ask)?;
        }
        Ok(())
    }
}

/// The Windows Credential Manager.
#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
mod os {
    use windows::core::{HRESULT, PCWSTR, PWSTR};
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn target(account: &str) -> Vec<u16> {
        wide(&format!("open-island/{account}"))
    }

    fn explain(e: windows::core::Error) -> String {
        format!("Credential Manager: {}", e.message())
    }

    fn missing(e: &windows::core::Error) -> bool {
        e.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0)
    }

    pub fn probe() -> Result<(), String> {
        read("probe").map(|_| ())
    }

    pub fn read(account: &str) -> Result<Option<String>, String> {
        let name = target(account);
        let mut found: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `name` ends in a zero and lives through the call; `found` is read only after a success and freed once
        unsafe {
            if let Err(e) = CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None, &mut found) {
                return if missing(&e) { Ok(None) } else { Err(explain(e)) };
            }
            let c = &*found;
            let bytes = if c.CredentialBlob.is_null() { vec![] } else { std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec() };
            CredFree(found as *const core::ffi::c_void);
            String::from_utf8(bytes).map(Some).map_err(|_| "Credential Manager: the secret is not text".to_string())
        }
    }

    pub fn write(account: &str, secret: &str) -> Result<(), String> {
        let mut name = target(account);
        let mut user = wide("open-island");
        let mut blob = secret.as_bytes().to_vec();
        let c = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(name.as_mut_ptr()),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(user.as_mut_ptr()),
            ..Default::default()
        };
        // SAFETY: every pointer in `c` refers to a buffer that lives through the call
        unsafe { CredWriteW(&c, 0) }.map_err(explain)
    }

    pub fn erase(account: &str) -> Result<(), String> {
        let name = target(account);
        // SAFETY: `name` ends in a zero and lives through the call
        match unsafe { CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None) } {
            Err(e) if !missing(&e) => Err(explain(e)),
            _ => Ok(()),
        }
    }
}

/// The macOS Keychain.
#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
mod os {
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

    const SERVICE: &str = "open-island";
    /// errSecItemNotFound
    const NOT_FOUND: i32 = -25300;

    fn explain(e: security_framework::base::Error) -> String {
        format!("Keychain: {e}")
    }

    pub fn probe() -> Result<(), String> {
        read("probe").map(|_| ())
    }

    pub fn read(account: &str) -> Result<Option<String>, String> {
        match get_generic_password(SERVICE, account) {
            Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| "Keychain: the secret is not text".to_string()),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(explain(e)),
        }
    }

    pub fn write(account: &str, secret: &str) -> Result<(), String> {
        set_generic_password(SERVICE, account, secret.as_bytes()).map_err(explain)
    }

    pub fn erase(account: &str) -> Result<(), String> {
        match delete_generic_password(SERVICE, account) {
            Err(e) if e.code() != NOT_FOUND => Err(explain(e)),
            _ => Ok(()),
        }
    }
}

#[cfg(all(not(test), not(any(target_os = "linux", windows, target_os = "macos"))))]
mod none {
    const WHY: &str = "no keyring on this system: use password_cmd";

    pub fn probe() -> Result<(), String> {
        Err(WHY.into())
    }

    pub fn read(_: &str) -> Result<Option<String>, String> {
        Err(WHY.into())
    }

    pub fn write(_: &str, _: &str) -> Result<(), String> {
        Err(WHY.into())
    }

    pub fn erase(_: &str) -> Result<(), String> {
        Err(WHY.into())
    }
}

/// The keyring of the tests. A secret equal to `refuse` is turned down, as a keyring that stays locked would.
#[cfg(test)]
mod memory {
    use std::collections::HashMap;
    use std::sync::Mutex;

    static HELD: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

    fn with<T>(f: impl FnOnce(&mut HashMap<String, String>) -> T) -> T {
        f(HELD.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new))
    }

    pub fn probe() -> Result<(), String> {
        Ok(())
    }

    pub fn read(account: &str) -> Result<Option<String>, String> {
        Ok(with(|m| m.get(account).cloned()))
    }

    pub fn write(account: &str, secret: &str) -> Result<(), String> {
        if secret == "refuse" {
            return Err("the keyring was not unlocked".into());
        }
        with(|m| m.insert(account.into(), secret.into()));
        Ok(())
    }

    pub fn erase(account: &str) -> Result<(), String> {
        with(|m| m.remove(account));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("open-island-secrets-{}-{name}", std::process::id())).join("config.json");
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn an_account_added_from_the_menu_leaves_no_password_in_the_file() {
        let p = file("add");
        apply(&p, &Command::AddICloud { user: "add@icloud.com".into(), password: "abcd-efgh".into() }).unwrap();
        apply(&p, &Command::AddCalDav { server: "https://s/dav/".into(), user: "add".into(), password: "hunter2".into() }).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("abcd-efgh") && !text.contains("hunter2") && !text.contains("password"), "{text}");
        assert_eq!(crate::config::parse(&text).unwrap().calendars.len(), 2);
        assert_eq!(get(&caldav_account(crate::config::ICLOUD_SERVER, "add@icloud.com")).unwrap().as_deref(), Some("abcd-efgh"));
        assert_eq!(get(&caldav_account("https://s/dav/", "add")).unwrap().as_deref(), Some("hunter2"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn a_keyring_that_refuses_changes_nothing() {
        let p = file("refused");
        assert!(apply(&p, &Command::AddICloud { user: "no@icloud.com".into(), password: "refuse".into() }).is_err());
        assert!(!p.exists(), "no account without its password, and no password in the file instead");
    }

    #[test]
    fn the_map_key_goes_to_the_keyring_and_is_removed_from_it() {
        let p = file("map");
        let set_key = |t: &str| Command::SetOption { key: "map_api_key".into(), value: OptionValue::Text(t.into()) };
        crate::config::update_at(&p, |o| {
            o.insert("map_api_key".into(), "old-plain".into());
        })
        .unwrap();
        apply(&p, &set_key(" k-123 ")).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("k-123") && !text.contains("old-plain"), "{text}");
        assert!(crate::config::parse(&text).unwrap().map_key_in_keyring);
        assert_eq!(cached(MAP_KEY).as_deref(), Some("k-123"));
        assert!(apply(&p, &set_key("two words")).is_err());
        apply(&p, &set_key("")).unwrap();
        assert!(!crate::config::parse(&std::fs::read_to_string(&p).unwrap()).unwrap().map_key_in_keyring);
        assert_eq!(cached(MAP_KEY), None);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// Needs a real Secret Service: run it on a private session bus with a keyring of its own.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "writes to the keyring of the session bus it runs on"]
    fn the_secret_service_stores_replaces_and_forgets() {
        let account = format!("test:{}", std::process::id());
        os::probe().unwrap();
        assert_eq!(os::read(&account).unwrap(), None);
        os::write(&account, "first è").unwrap();
        os::write(&account, "second").unwrap();
        assert_eq!(os::read(&account).unwrap().as_deref(), Some("second"));
        os::erase(&account).unwrap();
        assert_eq!(os::read(&account).unwrap(), None);
    }
}
