use miniquad::{debug, error, info, trace, warn};
use tracing::{
    field::{Field, Visit},
    Level, Subscriber,
};
use tracing_subscriber::{prelude::*, Layer};

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
        let mut msg = meta.target().to_owned();
        if !v.1.is_empty() {
            msg.push('{');
            let len = v.1.len();
            for (idx, (name, val)) in v.1.into_iter().enumerate() {
                use std::fmt::Write;
                let _ = write!(msg, "{name}={val}{}", if idx + 1 == len { '}' } else { ' ' });
            }
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
    tracing_subscriber::registry().with(CustomLayer).init();
}
