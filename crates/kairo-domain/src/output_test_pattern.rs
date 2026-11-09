//! The diagnostic test pattern layer, from `src/lib/output-test-pattern.ts`.

use crate::video_devices::VideoTestPattern;

/// `escapeHtml` from `src/lib/overlay-template.ts`, which prompt 051 ports in
/// full; it lives here until then.
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

const BAR_COLORS: [&str; 7] = [
    "#ffffff", "#ffff00", "#00ffff", "#00ff00", "#ff00ff", "#ff0000", "#0000ff",
];

/// `outputTestPatternHTML`: one removable layer above live content.
pub fn output_test_pattern_html(pattern: VideoTestPattern, name: &str) -> String {
    let body = match pattern {
        VideoTestPattern::Bars => {
            let bars: String = BAR_COLORS
                .iter()
                .map(|color| format!(r#"<div style="background:{color};flex:1"></div>"#))
                .collect();
            format!(
                r#"<div style="display:flex;height:72%">{bars}</div><div style="height:28%;background:linear-gradient(to right,#000,#fff)"></div>"#
            )
        }
        VideoTestPattern::Text => r#"<div style="padding:6%;line-height:1.5"><div style="font-size:64px">Kairo · Fine text and alignment</div><div style="font-size:36px">ABCDEFGHIJKLMNOPQRSTUVWXYZ · 0123456789</div><div style="font-size:24px">White text · sharp edges · even horizontal lines</div><div style="margin-top:40px;height:180px;background:repeating-linear-gradient(to bottom,#fff 0px,#fff 1px,#000 1px,#000 4px)"></div></div>"#.to_owned(),
        VideoTestPattern::Motion => r#"<style>@keyframes kairo-diagnostic-sweep{from{transform:translateX(0)}to{transform:translateX(1800px)}}</style><div style="position:absolute;left:0;top:0;width:120px;height:100%;background:#fff;animation:kairo-diagnostic-sweep 3s linear infinite alternate"></div><div style="position:absolute;inset:0;background:repeating-linear-gradient(to right,transparent 0,transparent 119px,#475569 119px,#475569 120px)"></div>"#.to_owned(),
    };
    let label = match pattern {
        VideoTestPattern::Bars => "Color bars",
        VideoTestPattern::Text => "Fine text",
        VideoTestPattern::Motion => "Motion",
    };
    format!(
        r#"<div id="kairo-diagnostic-pattern" style="position:fixed;inset:0;z-index:2147483647;background:#000;color:#fff;font-family:Arial,sans-serif;overflow:hidden;pointer-events:none">{body}<div style="position:absolute;bottom:24px;left:24px;padding:12px 18px;background:#000;color:#fff;font-size:24px">{} · {label} test</div></div>"#,
        escape_html(name)
    )
}
