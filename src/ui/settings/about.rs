//! About: product mark, version, host facts.
use super::*;

// ---- About ----

pub(super) fn page(host: &Host, sys: &dyn System) -> gtk::Box {
    let p = w::vbox(w::S3);
    p.add_css_class("page");
    let mark = w::app_icon("wolos-mark", w::ICON_XL);
    mark.set_halign(gtk::Align::Center);
    p.append(&mark);
    p.append(&w::centered("WolOS", "large-title"));
    p.append(&w::centered(
        &format!("Version {}", env!("CARGO_PKG_VERSION")),
        "secondary",
    ));
    let or = |s: &str| {
        if s.is_empty() {
            "Unknown".to_string()
        } else {
            s.to_string()
        }
    };
    let mut rows = vec![
        info("Name", &or(&host.hostname)),
        info("Base", "FreeBSD 15.1"),
    ];
    if !host.processor.is_empty() {
        rows.push(info("Processor", &host.processor));
    }
    if !host.memory.is_empty() {
        rows.push(info("Memory", &host.memory));
    }
    if let Some(b) = crate::system::battery_label(sys.battery()) {
        rows.push(info("Battery", &b));
    }
    p.append(&w::section("", &rows));
    p
}
