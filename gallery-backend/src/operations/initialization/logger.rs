use env_logger::{Builder, WriteStyle};
use log::kv::Key;
use std::io::Write;

pub fn initialize_logger() {
    Builder::new()
        .write_style(WriteStyle::Auto)
        .format(|buf, record| {
            let ts = buf.timestamp();
            let tgt = record.target();

            let level_style = buf.default_level_style(record.level());
            let level = format!(
                "{}{}{}",
                level_style.render(),
                record.level(),
                level_style.render_reset()
            );

            let dur_raw = record
                .key_values()
                .get(Key::from("duration"))
                .map(|v| {
                    let s = format!("{v}");
                    if let Some(idx) = s.find(|c: char| c.is_alphabetic()) {
                        let (num, unit) = (&s[..idx], &s[idx..]);
                        if let Ok(val) = num.parse::<f32>() {
                            return format!("{val:.2} {unit}");
                        }
                    }
                    s
                })
                .unwrap_or_default();

            let dur = if dur_raw.is_empty() {
                " ".repeat(10)
            } else {
                format!("{dur_raw:>10}")
            };

            writeln!(buf, "{ts} {level} {tgt}")?;

            let message = format!("{}", record.args());
            let subsequent_indent = " ".repeat(11);
            let mut lines = message.lines();
            if let Some(first_line) = lines.next() {
                writeln!(buf, "{dur} {first_line}")?;
            }
            for line in lines {
                writeln!(buf, "{subsequent_indent}{line}")?;
            }
            Ok(())
        })
        .filter(None, log::LevelFilter::Info)
        .filter(Some("rocket"), log::LevelFilter::Warn)
        .init();
}
