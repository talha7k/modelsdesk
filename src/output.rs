use console::style;

pub fn ok(s: &str) -> String {
    style(format!("✓ {s}")).green().to_string()
}

pub fn warn(s: &str) -> String {
    style(format!("! {s}")).yellow().to_string()
}

pub fn err(s: &str) -> String {
    style(format!("✗ {s}")).red().to_string()
}

pub fn dim(s: &str) -> String {
    style(s).dim().to_string()
}

pub fn bold(s: &str) -> String {
    style(s).bold().to_string()
}

pub fn cyan(s: &str) -> String {
    style(s).cyan().to_string()
}
