//! The content server's log (`src/log.ts`): one JSON object per line, `{t, level, msg, ...fields}`,
//! on stdout (stderr for warnings and errors), so the host can grep or load it. Nothing here ever
//! receives an account key: callers log the account id instead. Its own format, not pino's: the
//! content server never used pino, and the host's tools read these lines as they are.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug = 10,
    Info = 20,
    Warn = 30,
    Error = 40,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

type Sink = Box<dyn Fn(&str, Level) + Send + Sync>;

struct Logger {
    threshold: Level,
    sink: Option<Sink>,
}

fn logger() -> &'static Mutex<Logger> {
    static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();
    LOGGER.get_or_init(|| {
        let threshold = match std::env::var("LOG_LEVEL").unwrap_or_default().to_lowercase().as_str() {
            "debug" => Level::Debug,
            "warn" => Level::Warn,
            "error" => Level::Error,
            _ => Level::Info,
        };
        Mutex::new(Logger { threshold, sink: None })
    })
}

/// Tests capture the log instead of printing it.
pub fn set_sink(sink: Option<Sink>) {
    logger().lock().unwrap_or_else(std::sync::PoisonError::into_inner).sink = sink;
}

pub fn set_level(level: Level) {
    logger()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .threshold = level;
}

/// `Log`: `fields` must be a JSON object (anything else is left out).
pub fn log(level: Level, msg: &str, fields: Value) {
    let guard = logger().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if level < guard.threshold {
        return;
    }
    let mut line = Map::new();
    line.insert(
        "t".into(),
        json!(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
    );
    line.insert("level".into(), json!(level.as_str()));
    line.insert("msg".into(), json!(msg));
    if let Value::Object(fields) = fields {
        for (name, value) in fields {
            line.insert(name, value);
        }
    }
    let text = service_kit::jsjson::stringify(&Value::Object(line));
    if let Some(sink) = &guard.sink {
        sink(&text, level);
        return;
    }
    drop(guard);
    if level >= Level::Warn {
        let _ = writeln!(std::io::stderr(), "{text}");
    } else {
        let _ = writeln!(std::io::stdout(), "{text}");
    }
}

pub fn info(msg: &str, fields: Value) {
    log(Level::Info, msg, fields);
}

pub fn warn(msg: &str, fields: Value) {
    log(Level::Warn, msg, fields);
}

pub fn error(msg: &str, fields: Value) {
    log(Level::Error, msg, fields);
}
