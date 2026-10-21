use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Macos,
    Linux,
    Windows,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Arm64,
    X64,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxDistro {
    Debian, // apt (Debian/Ubuntu/Pop!_OS/…)
    Fedora, // dnf (Fedora/RHEL/CentOS/…)
    Arch,   // pacman (Arch/Manjaro/…)
    Suse,   // zypper (openSUSE/…)
    Alpine, // apk
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageManager {
    Brew,
    Pipx,
    Uv,
    Apt,
    Dnf,
    Pacman,
    Zypper,
    Apk,
    Winget,
    Choco,
    Scoop,
    Cargo,
}

#[derive(Debug, Clone)]
pub struct Env {
    pub os: Os,
    pub arch: Arch,
    pub os_version: Option<String>,
    pub distro: Option<LinuxDistro>,
    pub managers: Vec<PackageManager>,
}

impl Env {
    pub fn detect() -> Self {
        let os = if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Unknown
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else if cfg!(target_arch = "x86_64") {
            Arch::X64
        } else {
            Arch::Other
        };
        let distro = if os == Os::Linux {
            Some(detect_distro())
        } else {
            None
        };
        let managers = detect_managers(os);
        let os_version = os_version(os);
        Self {
            os,
            arch,
            os_version,
            distro,
            managers,
        }
    }

    pub fn has(&self, pm: PackageManager) -> bool {
        self.managers.contains(&pm)
    }

    pub fn label(&self) -> String {
        let mut s = match self.os {
            Os::Macos => "macOS".to_string(),
            Os::Linux => format!(
                "Linux{}",
                self.distro
                    .as_ref()
                    .map(|d| format!("/{d:?}").to_lowercase())
                    .unwrap_or_default()
            ),
            Os::Windows => "Windows".to_string(),
            Os::Unknown => "unknown OS".to_string(),
        };
        if let Some(v) = &self.os_version {
            s.push_str(&format!(" {v}"));
        }
        s.push_str(match self.arch {
            Arch::Arm64 => " (arm64)",
            Arch::X64 => " (x64)",
            Arch::Other => "",
        });
        s
    }

    pub fn supports_mlx(&self) -> bool {
        self.os == Os::Macos && self.arch == Arch::Arm64 && macos_at_least(self, 14, 0)
    }

    pub fn supports(&self, backend: &str) -> bool {
        match backend {
            "mlx-dspark" | "mlx-lm" => self.supports_mlx(),
            "llama-cpp" | "ollama" | "strata" | "mistral-rs" | "vllm" | "sglang" | "tabby" => {
                !matches!(self.os, Os::Unknown)
            }
            _ => false,
        }
    }
}

fn detect_distro() -> LinuxDistro {
    let content = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let lower = content.to_lowercase();
    let id = lower
        .lines()
        .find(|l| l.starts_with("ID="))
        .map(|l| l.trim_start_matches("ID=").trim_matches('"').to_string())
        .unwrap_or_default();
    let like = lower
        .lines()
        .find(|l| l.starts_with("ID_LIKE="))
        .map(|l| l.to_string())
        .unwrap_or_default();
    let matches = |names: &[&str]| {
        names.iter().any(|n| id == *n) || names.iter().any(|n| like.contains(n))
    };
    if matches(&["debian", "ubuntu", "pop", "linuxmint", "raspbian"]) {
        LinuxDistro::Debian
    } else if matches(&["fedora", "rhel", "centos", "rocky", "almalinux"]) {
        LinuxDistro::Fedora
    } else if matches(&["arch", "manjaro", "endeavouros"]) {
        LinuxDistro::Arch
    } else if matches(&["suse", "opensuse"]) {
        LinuxDistro::Suse
    } else if id == "alpine" {
        LinuxDistro::Alpine
    } else {
        LinuxDistro::Unknown
    }
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|d| {
                d.join(name).is_file()
                    || d.join(format!("{name}.exe")).is_file()
            })
        })
        .unwrap_or(false)
}

fn detect_managers(os: Os) -> Vec<PackageManager> {
    let mut out = Vec::new();
    let mut probe = |pm: PackageManager, bins: &[&str]| {
        if bins.iter().any(|b| on_path(b)) {
            out.push(pm);
        }
    };
    match os {
        Os::Macos => {
            probe(PackageManager::Brew, &["brew"]);
            probe(PackageManager::Pipx, &["pipx"]);
            probe(PackageManager::Uv, &["uv", "uvx"]);
            probe(PackageManager::Cargo, &["cargo"]);
        }
        Os::Linux => {
            probe(PackageManager::Brew, &["brew"]);
            probe(PackageManager::Pipx, &["pipx"]);
            probe(PackageManager::Uv, &["uv", "uvx"]);
            probe(PackageManager::Apt, &["apt-get"]);
            probe(PackageManager::Dnf, &["dnf"]);
            probe(PackageManager::Pacman, &["pacman"]);
            probe(PackageManager::Zypper, &["zypper"]);
            probe(PackageManager::Apk, &["apk"]);
            probe(PackageManager::Cargo, &["cargo"]);
        }
        Os::Windows => {
            probe(PackageManager::Winget, &["winget"]);
            probe(PackageManager::Choco, &["choco"]);
            probe(PackageManager::Scoop, &["scoop"]);
            probe(PackageManager::Uv, &["uv", "uvx"]);
            probe(PackageManager::Cargo, &["cargo"]);
        }
        Os::Unknown => {}
    }
    out
}

#[cfg(target_os = "macos")]
fn os_version(_os: Os) -> Option<String> {
    std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "linux")]
fn os_version(_os: Os) -> Option<String> {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("VERSION_ID="))
                .map(|l| {
                    l.trim_start_matches("VERSION_ID=")
                        .trim_matches('"')
                        .to_string()
                })
        })
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "windows")]
fn os_version(_os: Os) -> Option<String> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn os_version(_os: Os) -> Option<String> {
    None
}

fn parse_version(v: &str) -> Option<(u64, u64)> {
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

fn macos_at_least(env: &Env, major: u64, minor: u64) -> bool {
    if env.os != Os::Macos {
        return false;
    }
    match env.os_version.as_deref().and_then(parse_version) {
        Some((ma, mi)) => (ma, mi) >= (major, minor),
        None => true,
    }
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}
