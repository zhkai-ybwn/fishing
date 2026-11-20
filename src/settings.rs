use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use boss_sensor_platform::WindowTarget;
use boss_sensor_vision::{EMBEDDING_LEN, EMBEDDING_MODEL};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_NOTIFICATION_TEXT: &str = "已专注{时长}，起身走走，休息一下吧。";

#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Settings {
    version: u32,
    pub camera_id: Option<String>,
    pub preview: bool,
    #[serde(alias = "second_face")]
    pub face_count_enabled: bool,
    pub minimum_faces: u32,
    pub recognize: bool,
    pub notify: bool,
    pub notification_text: String,
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
            face_count_enabled: false,
            minimum_faces: 2,
            recognize: false,
            notify: true,
            notification_text: DEFAULT_NOTIFICATION_TEXT.into(),
            switch_window: false,
            target: None,
            people: Vec::new(),
        }
    }
}

impl Settings {
    pub fn notification_message(&self) -> Option<&str> {
        self.notify.then(|| {
            let text = self.notification_text.trim();
            if text.is_empty() {
                DEFAULT_NOTIFICATION_TEXT
            } else {
                text
            }
        })
    }
}

pub fn render_reminder(template: &str, elapsed: Duration) -> String {
    if !template.contains("{时长}") {
        return template.to_owned();
    }
    let minutes = elapsed.as_secs() / 60;
    let duration = match (minutes / 60, minutes % 60) {
        (0, 0) => "不到 1 分钟".into(),
        (0, minutes) => format!("{minutes} 分钟"),
        (hours, 0) => format!("{hours} 小时"),
        (hours, minutes) => format!("{hours} 小时 {minutes} 分钟"),
    };
    template.replace("{时长}", &duration)
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
        // Keep the original data directory so renamed clients load existing settings.
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
            .map_err(|error| format!("无法启动，可能已有 fishing 实例运行 · {error}"))?;
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
        let mut settings: Settings =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if settings.version != SCHEMA_VERSION {
            return Err(format!("此版本无法读取版本 {} 的设置", settings.version));
        }
        settings.minimum_faces = settings.minimum_faces.max(1);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_count_rule_keeps_its_threshold_and_gets_a_discreet_message() {
        let settings: Settings =
            serde_json::from_str(r#"{"version":1,"second_face":true}"#).unwrap();
        assert!(settings.face_count_enabled);
        assert_eq!(settings.minimum_faces, 2);
        assert_eq!(
            settings.notification_message(),
            Some(DEFAULT_NOTIFICATION_TEXT)
        );
    }

    #[test]
    fn notification_text_handles_custom_blank_and_disabled_values() {
        let mut settings = Settings {
            notification_text: "  喝水时间  ".into(),
            ..Settings::default()
        };
        assert_eq!(settings.notification_message(), Some("喝水时间"));
        settings.notification_text.clear();
        assert_eq!(
            settings.notification_message(),
            Some(DEFAULT_NOTIFICATION_TEXT)
        );
        settings.notify = false;
        assert_eq!(settings.notification_message(), None);
    }

    #[test]
    fn reminder_substitutes_the_elapsed_session_time() {
        assert_eq!(
            render_reminder("休息{时长}后继续", Duration::ZERO),
            "休息不到 1 分钟后继续"
        );
        assert_eq!(
            render_reminder("已专注{时长}", Duration::from_secs(25 * 60)),
            "已专注25 分钟"
        );
        assert_eq!(
            render_reminder("已专注{时长}", Duration::from_secs(65 * 60)),
            "已专注1 小时 5 分钟"
        );
        assert_eq!(
            render_reminder("喝水时间", Duration::from_secs(15 * 60)),
            "喝水时间"
        );
    }
}
