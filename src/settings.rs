use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;

use boss_sensor_platform::WindowTarget;
use boss_sensor_vision::{EMBEDDING_LEN, EMBEDDING_MODEL};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Settings {
    version: u32,
    pub camera_id: Option<String>,
    pub preview: bool,
    pub second_face: bool,
    pub recognize: bool,
    pub notify: bool,
    pub switch_window: bool,
    pub target: Option<WindowTarget>,
    pub people: Vec<Person>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            camera_id: None,
            preview: true,
            second_face: false,
            recognize: false,
            notify: true,
            switch_window: false,
            target: None,
            people: Vec::new(),
        }
    }
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub struct Person {
    pub name: String,
    model: String,
    embedding: Vec<f32>,
}

impl Person {
    pub fn new(name: String, embedding: Vec<f32>) -> Self {
        Self {
            name,
            model: EMBEDDING_MODEL.to_owned(),
            embedding,
        }
    }

    pub fn embedding(&self) -> Option<&[f32]> {
        (self.model == EMBEDDING_MODEL
            && self.embedding.len() == EMBEDDING_LEN
            && self.embedding.iter().all(|value| value.is_finite()))
        .then_some(self.embedding.as_slice())
    }
}

pub struct Storage {
    path: PathBuf,
    _instance_lock: File,
}

impl Storage {
    pub fn open() -> Result<Self, String> {
        let local_data = std::env::var_os("LOCALAPPDATA").ok_or("找不到 Windows 用户数据目录")?;
        let directory = PathBuf::from(local_data).join("BossSensor");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("instance.lock"))
            .map_err(|error| error.to_string())?;
        lock.try_lock()
            .map_err(|error| format!("无法启动，可能已有 BossSensor 实例运行 · {error}"))?;
        Ok(Self {
            path: directory.join("settings.json"),
            _instance_lock: lock,
        })
    }

    pub fn load(&self) -> Result<Settings, String> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(error) => return Err(error.to_string()),
        };
        let settings: Settings =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if settings.version != SCHEMA_VERSION {
            return Err(format!("此版本无法读取版本 {} 的设置", settings.version));
        }
        Ok(settings)
    }

    pub fn save(&self, settings: &Settings) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
        let temp = self.path.with_extension("tmp");
        {
            let mut file = File::create(&temp).map_err(|error| error.to_string())?;
            file.write_all(&bytes).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
        }
        fs::rename(temp, &self.path).map_err(|error| error.to_string())
    }
}
