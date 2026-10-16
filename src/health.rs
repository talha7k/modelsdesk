#[derive(Debug, Clone)]
pub struct HealthReport {
    pub ok: bool,
    pub status_line: String,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub detail: String,
}
