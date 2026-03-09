//! Logging utilities.

use colored::Colorize;
use miniquad::{debug, error, info, trace, warn};
use tracing::{field::Visit, Level, Subscriber};
use tracing_subscriber::{filter, prelude::*, Layer};

struct CustomLayer;

impl<S> Layer<S> for CustomLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        #[derive(Default)]
        struct Visitor {
            message: Option<String>,
            target: Option<String>,
            fields: Vec<(&'static str, String)>,
        }
        impl Visit for Visitor {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "message" {
                    self.message = Some(value.to_string());
                } else if !field.name().starts_with("log.") {
                    self.fields.push((field.name(), value.to_string()));
                } else if field.name() == "log.target" {
                    self.target = Some(value.to_string());
                }
            }

            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                let val = format!("{value:?}");
                if field.name() == "message" {
                    self.message = Some(val);
                } else if !field.name().starts_with("log.") {
                    self.fields.push((field.name(), val));
                }
            }
        }

        let mut v = Visitor::default();
        event.record(&mut v);

        let meta = event.metadata();
        let target = v.target.as_deref().unwrap_or_else(|| meta.target());
        if target.starts_with("jni::") && meta.level() >= &Level::INFO {
            return;
        }

        #[cfg(not(target_os = "android"))]
        let mut msg = format!("{:.6?} ", chrono::Utc::now()).bright_black().to_string()
            + &match *meta.level() {
                Level::TRACE => "TRACE".bright_black(),
                Level::DEBUG => "DEBUG".magenta(),
                Level::INFO => " INFO".green(),
                Level::WARN => " WARN".yellow(),
                Level::ERROR => "ERROR".red(),
            }
            .to_string()
            + " ";

        #[cfg(target_os = "android")]
        let mut msg = String::new();

        msg += &target.bright_black().to_string();
        if !v.fields.is_empty() {
            msg += &"{".bold().to_string();
            for (name, val) in &v.fields {
                use std::fmt::Write;
                let _ = write!(msg, "{}={val} ", name.italic());
            }
            if !v.fields.is_empty() {
                msg.pop();
            }
            msg += &"}".bold().to_string();
        }
        if let Some(message) = v.message {
            msg += ": ";
            msg += &message;
        }

        match *meta.level() {
            Level::TRACE => trace!("{}", msg),
            Level::DEBUG => debug!("{}", msg),
            Level::INFO => info!("{}", msg),
            Level::WARN => warn!("{}", msg),
            Level::ERROR => error!("{}", msg),
        }
    }
}

pub fn register() {
    tracing_subscriber::registry()
        .with(CustomLayer)
        .with(
            filter::Targets::new()
                .with_target("hyper", Level::INFO)
                .with_target("rustls", Level::INFO)
                .with_default(Level::TRACE),
        )
        .init();
}
