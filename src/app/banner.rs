use crate::config::Config;
use if_addrs::{IfOperStatus, Interface, get_if_addrs};
use std::{
    io::IsTerminal,
    net::{IpAddr, SocketAddr},
    path::Path,
    time::Duration,
};

/// Helper struct for terminal styling and ANSI color control.
struct Styler {
    color_enabled: bool,
}

impl Styler {
    fn new() -> Self {
        Self {
            color_enabled: std::io::stdout().is_terminal(),
        }
    }

    fn rgb(&self, text: &str, r: u8, g: u8, b: u8) -> String {
        if self.color_enabled {
            format!("\x1b[38;2;{r};{g};{b}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn bold_rgb(&self, text: &str, r: u8, g: u8, b: u8) -> String {
        if self.color_enabled {
            format!("\x1b[1;38;2;{r};{g};{b}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn dim(&self, text: &str) -> String {
        if self.color_enabled {
            format!("\x1b[2;38;2;148;163;184m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

/// Calculate visible character count, ignoring ANSI escape sequences.
fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

/// Shorten path for display if inside current working directory
fn display_path(path: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(rel) = path.strip_prefix(&cwd)
    {
        return rel.display().to_string();
    }
    path.display().to_string()
}

fn collect_host_ips(interfaces: impl IntoIterator<Item = Interface>) -> Vec<IpAddr> {
    let interfaces: Vec<_> = interfaces
        .into_iter()
        .filter(|interface| !interface.is_loopback() && !interface.is_link_local())
        .filter(|interface| {
            let ip = interface.ip();
            !ip.is_unspecified() && !ip.is_multicast()
        })
        .collect();
    let has_up_interface = interfaces.iter().any(Interface::is_oper_up);
    let has_unknown_interface = !has_up_interface
        && interfaces
            .iter()
            .any(|interface| interface.oper_status == IfOperStatus::Unknown);
    let mut ips: Vec<_> = interfaces
        .into_iter()
        .filter(|interface| {
            interface.is_oper_up()
                || (has_unknown_interface && interface.oper_status == IfOperStatus::Unknown)
        })
        .map(|interface| interface.ip())
        .collect();
    ips.sort_by_key(|ip| (if ip.is_ipv4() { 0 } else { 1 }, ip.to_string()));
    ips.dedup();
    ips
}

fn host_ips() -> Vec<IpAddr> {
    get_if_addrs().map(collect_host_ips).unwrap_or_default()
}

/// Print the beautiful ExoRoute startup screen
pub fn print_startup(
    addr: SocketAddr,
    elapsed: Duration,
    config: &Config,
    must_change_password: bool,
) {
    let s = Styler::new();

    println!();

    // ─── 1. TOP WING TIPS ───────────────────────────────────────────
    let fish_1 = format!("        {}      ", s.rgb(r"\   /", 34, 211, 238));
    println!("{fish_1}");

    // ─── 2. UPPER WING SWEEP + EXOROUTE LINE 1 ───────────────────────
    let fish_2 = format!("      {}    ", s.rgb(r"___\ /___", 56, 189, 248));
    let text_2_exo = s.rgb("   ____             ", 192, 132, 252);
    let text_2_route = s.rgb("____             __       ", 96, 165, 250);
    println!("{fish_2}  {text_2_exo}{text_2_route}");

    // ─── 3. EXPANDED WINGS + EXOROUTE LINE 2 ────────────────────────
    let fish_3 = format!(
        "     {}    {}    {}   ",
        s.rgb("/", 56, 189, 248),
        s.bold_rgb("V", 103, 232, 249),
        s.rgb(r"\", 56, 189, 248)
    );
    let text_3_exo = s.rgb(r"  / __/  ______    ", 168, 85, 247);
    let text_3_route = s.rgb(r"/ __ \____  __  __/ /____  ", 56, 189, 248);
    println!("{fish_3}  {text_3_exo}{text_3_route}");

    // ─── 4. BODY & GLOWING EYE + EXOROUTE LINE 3 ────────────────────
    let fish_4 = format!(
        "    {}   {} {} {}   {}  ",
        s.rgb("(", 129, 140, 248),
        s.rgb("(", 99, 102, 241),
        s.bold_rgb("o", 255, 255, 255),
        s.rgb(")", 99, 102, 241),
        s.rgb(")", 129, 140, 248)
    );
    let text_4_exo = s.rgb(r" / _/ | |/_/ _ \  ", 147, 51, 234);
    let text_4_route = s.rgb(r"/ /_/ / __ \/ / / / __/ _ \ ", 34, 211, 238);
    println!("{fish_4}  {text_4_exo}{text_4_route}");

    // ─── 5. LOWER FINS & RAYS + EXOROUTE LINE 4 ─────────────────────
    let fish_5 = format!(
        "     {}  {} {} {}  {}   ",
        s.rgb(r"\", 129, 140, 248),
        s.rgb("/", 99, 102, 241),
        s.rgb("|", 147, 51, 234),
        s.rgb(r"\", 99, 102, 241),
        s.rgb("/", 129, 140, 248)
    );
    let text_5_exo = s.rgb(r"/ /___>  </ (_)  ", 126, 34, 206);
    let text_5_route = s.rgb(r"/ _, _/ /_/ / /_/ / /_/  __/ ", 6, 182, 212);
    println!("{fish_5}  {text_5_exo}{text_5_route}");

    // ─── 6. FIN TAPER + EXOROUTE LINE 5 ─────────────────────────────
    let fish_6 = format!(
        "      {}  {}  {}    ",
        s.rgb(r"\/", 147, 51, 234),
        s.rgb("|", 126, 34, 206),
        s.rgb(r"\/", 147, 51, 234)
    );
    let text_6_exo = s.rgb(r"\_____/_/|_|\___/ ", 107, 33, 168);
    let text_6_route = s.rgb(r"/_/ |_|\____/\__,_/\__/\___/  ", 14, 165, 233);
    println!("{fish_6}  {text_6_exo}{text_6_route}");

    // ─── 7. TAIL FIN + VERSION BADGE ────────────────────────────────
    let fish_7 = format!("          {}        ", s.bold_rgb("▼", 168, 85, 247));
    let version_badge = s.bold_rgb(concat!("v", env!("CARGO_PKG_VERSION")), 52, 211, 153);
    println!("{fish_7}                                                {version_badge}");

    // Subtitle tagline
    let tagline = s.dim("               Lightweight AI Protocol Gateway & Router");
    println!("{tagline}\n");

    // ─── STATUS CARD ────────────────────────────────────────────────
    let border_color = (71, 85, 105); // slate-600
    let box_width = 64;

    let border_top = format!(
        "  {}",
        s.rgb(
            &format!("╭{}╮", "─".repeat(box_width)),
            border_color.0,
            border_color.1,
            border_color.2
        )
    );
    let border_bottom = format!(
        "  {}",
        s.rgb(
            &format!("╰{}╮", "─".repeat(box_width)).replace('╮', "╯"),
            border_color.0,
            border_color.1,
            border_color.2
        )
    );
    let border_sep = format!(
        "  {}",
        s.rgb(
            &format!("├{}┤", "─".repeat(box_width)),
            border_color.0,
            border_color.1,
            border_color.2
        )
    );

    let format_row = |content: &str| -> String {
        let v_len = visible_len(content);
        let pad = if box_width >= v_len + 2 {
            box_width - v_len - 2
        } else {
            0
        };
        let b = s.rgb("│", border_color.0, border_color.1, border_color.2);
        format!("  {b}  {content}{}{b}", " ".repeat(pad))
    };

    let empty_row = format_row("");

    // Host & URLs
    let host_str = if addr.ip().is_unspecified() {
        "127.0.0.1".to_string()
    } else {
        addr.ip().to_string()
    };
    let base_url = format!("http://{}:{}", host_str, addr.port());
    let gateway_url_colored = s.bold_rgb(&base_url, 56, 189, 248);
    let dashboard_url_colored = s.bold_rgb(&format!("{base_url}/overview"), 192, 132, 252);

    // Row 1: Gateway URL
    let row_gateway = format!(
        "{}  {:<10}  {}",
        s.bold_rgb("●", 74, 222, 128),
        s.bold_rgb("Gateway", 248, 250, 252),
        gateway_url_colored
    );

    // Row 2: Dashboard URL
    let row_dashboard = format!(
        "{}  {:<10}  {}",
        s.bold_rgb("➜", 168, 85, 247),
        s.bold_rgb("Dashboard", 248, 250, 252),
        dashboard_url_colored
    );

    let host_ips = host_ips();

    // Row 3: Engine speed & mode
    let ms = elapsed.as_millis();
    let speed_colored = if ms < 200 {
        s.bold_rgb(&format!("{ms} ms"), 74, 222, 128)
    } else {
        s.bold_rgb(&format!("{ms} ms"), 250, 204, 21)
    };
    let row_engine = format!(
        "{}  {:<10}  Ready in {} {}",
        s.rgb("⚡", 250, 204, 21),
        s.rgb("Engine", 203, 213, 225),
        speed_colored,
        s.dim("(Rust / Axum)")
    );

    // Row 4: Database path
    let db_str = display_path(&config.database_path);
    let row_db = format!(
        "{}  {:<10}  {}",
        s.rgb("⛁", 96, 165, 250),
        s.rgb("Database", 203, 213, 225),
        s.rgb(&db_str, 148, 163, 184)
    );

    // Row 5: Security / Management
    let auth_status = if must_change_password {
        format!(
            "{} {}",
            s.bold_rgb("▲", 251, 191, 36),
            s.rgb("Default password active (change in Settings)", 251, 191, 36)
        )
    } else if config.admin_key.is_some() {
        format!(
            "{} {}",
            s.bold_rgb("✔", 74, 222, 128),
            s.rgb("Admin password configured", 148, 163, 184)
        )
    } else {
        format!(
            "{} {}",
            s.bold_rgb("○", 248, 113, 113),
            s.rgb("Open access (no admin key)", 248, 113, 113)
        )
    };
    let row_security = format!(
        "{}  {:<10}  {}",
        s.rgb("🔒", 251, 191, 36),
        s.rgb("Security", 203, 213, 225),
        auth_status
    );

    println!("{border_top}");
    println!("{empty_row}");
    println!("{}", format_row(&row_gateway));
    println!("{}", format_row(&row_dashboard));
    if host_ips.is_empty() {
        let row_host_ip = format!(
            "{}  {:<10}  {}",
            s.rgb("↗", 96, 165, 250),
            s.rgb("Host IP", 203, 213, 225),
            s.dim("Not detected")
        );
        println!("{}", format_row(&row_host_ip));
    } else {
        for (index, ip) in host_ips.iter().take(3).enumerate() {
            let label = if index == 0 { "Host IP" } else { "" };
            let row_host_ip = format!(
                "{}  {:<10}  {}",
                s.rgb("↗", 96, 165, 250),
                s.rgb(label, 203, 213, 225),
                s.bold_rgb(&ip.to_string(), 96, 165, 250)
            );
            println!("{}", format_row(&row_host_ip));
        }
        if host_ips.len() > 3 {
            let row_more_host_ips = format!(
                "{}  {:<10}  {}",
                "",
                "",
                s.dim(&format!("+ {} more addresses", host_ips.len() - 3))
            );
            println!("{}", format_row(&row_more_host_ips));
        }
    }
    println!("{empty_row}");
    println!("{border_sep}");
    println!("{empty_row}");
    println!("{}", format_row(&row_engine));
    println!("{}", format_row(&row_db));
    println!("{}", format_row(&row_security));
    println!("{empty_row}");
    println!("{border_bottom}");

    println!(
        "  {}\n",
        s.dim("• AI gateway ready to accept traffic · Press Ctrl+C to stop")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use if_addrs::{IfAddr, IfOperStatus, Ifv4Addr, Ifv6Addr};
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn interface(ip: IpAddr, oper_status: IfOperStatus) -> Interface {
        let addr = match ip {
            IpAddr::V4(ip) => IfAddr::V4(Ifv4Addr {
                ip,
                netmask: Ipv4Addr::UNSPECIFIED,
                prefixlen: 0,
                broadcast: None,
            }),
            IpAddr::V6(ip) => IfAddr::V6(Ifv6Addr {
                ip,
                netmask: Ipv6Addr::UNSPECIFIED,
                prefixlen: 0,
                broadcast: None,
            }),
        };
        Interface {
            name: "test-interface".to_owned(),
            addr,
            index: None,
            oper_status,
            #[cfg(windows)]
            adapter_name: "test-adapter".to_owned(),
        }
    }

    #[test]
    fn host_ip_list_filters_non_host_addresses_and_prefers_ipv4() {
        let ips = collect_host_ips([
            interface(IpAddr::V6("fd00::10".parse().unwrap()), IfOperStatus::Up),
            interface(IpAddr::V4(Ipv4Addr::LOCALHOST), IfOperStatus::Up),
            interface(IpAddr::V4(Ipv4Addr::UNSPECIFIED), IfOperStatus::Up),
            interface(
                IpAddr::V4("169.254.1.10".parse().unwrap()),
                IfOperStatus::Up,
            ),
            interface(
                IpAddr::V4("192.168.1.20".parse().unwrap()),
                IfOperStatus::Up,
            ),
            interface(
                IpAddr::V4("192.168.1.10".parse().unwrap()),
                IfOperStatus::Up,
            ),
            interface(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 1)), IfOperStatus::Up),
        ]);

        assert_eq!(
            ips,
            vec![
                IpAddr::V4("192.168.1.10".parse().unwrap()),
                IpAddr::V4("192.168.1.20".parse().unwrap()),
                IpAddr::V6("fd00::10".parse().unwrap()),
            ]
        );
    }

    #[test]
    fn host_ip_list_ignores_down_interfaces_when_an_up_address_exists() {
        let ips = collect_host_ips([
            interface(
                IpAddr::V4("192.168.1.10".parse().unwrap()),
                IfOperStatus::Up,
            ),
            interface(IpAddr::V4("10.0.0.20".parse().unwrap()), IfOperStatus::Down),
        ]);

        assert_eq!(ips, vec![IpAddr::V4("192.168.1.10".parse().unwrap())]);
    }

    #[test]
    fn host_ip_list_does_not_report_only_down_interfaces() {
        let ips = collect_host_ips([interface(
            IpAddr::V4("10.0.0.20".parse().unwrap()),
            IfOperStatus::Down,
        )]);

        assert!(ips.is_empty());
    }
}
