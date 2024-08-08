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
        struct Visitor(Option<String>, Vec<(&'static str, String)>);
        impl Visit for Visitor {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                let val = format!("{value:?}");
                if field.name() == "message" {
                    self.0 = Some(val);
                } else if !field.name().starts_with("log.") {
                    self.1.push((field.name(), val));
                }
            }
        }

        let mut v = Visitor(None, Vec::new());
        event.record(&mut v);

        let meta = event.metadata();

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

        msg += &meta.target().bright_black().to_string();
        if !v.1.is_empty() {
            msg += &"{".bold().to_string();
            let len = v.1.len();
            for (idx, (name, val)) in v.1.into_iter().enumerate() {
                use std::fmt::Write;
                let _ = write!(msg, "{}={val}", name.italic());
                if idx + 1 != len {
                    msg.push(' ');
                }
            }
            msg += &"}".bold().to_string();
        }
        if let Some(content) = v.0 {
            msg += ": ";
            msg += &content;
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
