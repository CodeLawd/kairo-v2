use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use kairo_store::{
    secrets::{KeyProtector, SecureStore, retire_provider_keys},
    settings::SettingsStore,
};
use serde::Serialize;
use serde_json::{Value, json};
use sysinfo::System;

const REFRESH_TOKEN: &str = "refresh-token";

pub(crate) type Settings = Arc<Mutex<SettingsStore>>;

#[derive(Clone)]
pub(crate) struct Profile {
    pub root: PathBuf,
    pub protector: Arc<dyn KeyProtector>,
    pub settings: Option<Settings>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Device {
    name: String,
    hostname: String,
    pub(crate) os: &'static str,
    pub(crate) os_version: String,
    pub(crate) arch: &'static str,
    pub(crate) app_version: &'static str,
    pub(crate) electron_version: &'static str,
}

impl Profile {
    pub fn load(&self) -> (String, Device, bool, Option<String>) {
        if let Some(settings) = &self.settings
            && let Ok(mut settings) = settings.lock()
        {
            let mut data = settings.data().clone();
            if retire_provider_keys(&mut data) {
                for name in ["stt", "lyrics"] {
                    if let Some(value) = data.remove(name) {
                        settings.set_raw(name, value);
                    }
                }
            }
        }

        let path = self.root.join("cloud/device.json");
        let id = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| {
                value
                    .get("deviceId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| {
                let id = uuid::Uuid::new_v4().to_string();
                let encoded = json!({ "deviceId": &id }).to_string();
                let _ = kairo_store::fs_atomic::write_atomic(&path, encoded.as_bytes());
                id
            });

        let os = match std::env::consts::OS {
            "macos" => "darwin",
            "windows" => "win32",
            other => other,
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => "x64",
            "x86" => "ia32",
            "aarch64" => "arm64",
            other => other,
        };
        let hostname = System::host_name().unwrap_or_default();
        let name = hostname
            .get(..hostname.len().saturating_sub(6))
            .filter(|_| hostname.to_ascii_lowercase().ends_with(".local"))
            .unwrap_or(&hostname);
        let name = if name.is_empty() {
            if os == "darwin" {
                "Mac · Kairo"
            } else {
                "PC · Kairo"
            }
        } else {
            name
        }
        .to_owned();
        let device = Device {
            name,
            hostname,
            os,
            arch,
            os_version: System::os_version()
                .or_else(System::kernel_version)
                .unwrap_or_default(),
            app_version: env!("CARGO_PKG_VERSION"),
            electron_version: "",
        };
        let vault = self.vault();
        let token = vault.read(REFRESH_TOKEN).filter(|token| !token.is_empty());
        (id, device, vault.credentials_notice().is_none(), token)
    }

    fn vault(&self) -> SecureStore<'_> {
        SecureStore::new(&self.root, self.protector.as_ref(), "Kairo")
    }

    pub fn persist(&self, token: Option<&str>) -> bool {
        let vault = self.vault();
        match token {
            Some(token) => vault.write(REFRESH_TOKEN, token),
            None => {
                vault.clear(REFRESH_TOKEN);
                vault.available()
            }
        }
    }

    pub fn seed_church(&self, name: &str) -> bool {
        let name = kairo_domain::js::trim(name);
        let Some(settings) = &self.settings else {
            return false;
        };
        let Ok(mut settings) = settings.lock() else {
            return false;
        };
        if name.is_empty() {
            return false;
        }
        let mut church = kairo_store::settings::spread(settings.data().get("church"));
        if church
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|current| !kairo_domain::js::trim(current).is_empty())
        {
            return false;
        }
        church.insert("name".into(), json!(name));
        settings.set_raw("church", Value::Object(church));
        true
    }
}

impl Device {
    pub fn payload(&self, id: Option<&str>, body: Value) -> Value {
        let mut body = body.as_object().cloned().unwrap_or_default();
        if let Some(id) = id {
            body.insert("deviceId".into(), json!(id));
        }
        body.insert("deviceName".into(), json!(self.name));
        body.insert("device".into(), json!(self));
        Value::Object(body)
    }
}
