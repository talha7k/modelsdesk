// These tests exercise live-process detection. They pass when a stray
// mlx-dspark serve on :8123 is running (as during development); otherwise
// they exercise the empty case.
#[test]
fn stray_detection_never_panics() {
    let cfg = modelsdesk::config::Config::default();
    let found = modelsdesk::server::strays(&cfg);
    // Every detected stray must have a pid and a truncated cmdline.
    for (pid, port, cmd) in &found {
        assert!(*pid > 0);
        assert!(port <= &65535);
        assert!(!cmd.is_empty());
        assert!(cmd.chars().count() <= 80);
    }
}

#[test]
fn port_parsing_rules() {
    // These are pure-logic checks mirrored from server.rs.
    let parse = |cmd: &str| -> Option<u16> {
        let mut it = cmd.split_whitespace();
        while let Some(tok) = it.next() {
            if tok == "--port" || tok == "-p" {
                if let Some(v) = it.next() {
                    if let Ok(p) = v.parse::<u16>() {
                        return Some(p);
                    }
                }
            }
        }
        None
    };
    assert_eq!(parse("serve --model x --port 8123"), Some(8123));
    assert_eq!(parse("serve -p 9999"), Some(9999));
    assert_eq!(parse("serve --model x"), None);
    // First --port wins (never the model name).
    assert_eq!(parse("serve --port 8080 --ctx 4096"), Some(8080));
}
