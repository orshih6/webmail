//! Message HTML → a self-contained document for a sandboxed iframe.
//!
//! Defence in depth: ammonia removes scripts, event handlers and dangerous URLs here, and
//! the UI renders the result in `<iframe sandbox>` without `allow-scripts`. Remote resources
//! (images, CSS `url()`) are stripped unless the user asked for them, so opening a message
//! does not tell a tracker anything.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const FRAME_STYLE: &str = "html,body{margin:0;padding:12px;font:14px/1.5 system-ui,sans-serif;\
color:#1f2328;background:#fff;overflow-wrap:anywhere}img{max-width:100%;height:auto}\
pre{white-space:pre-wrap}blockquote{margin:0 0 0 .5em;padding-left:.75em;border-left:3px solid #d0d7de}";

/// Returns the full HTML document and whether any remote resource was removed.
pub fn document(html: &str, cids: &HashMap<String, String>, allow_remote: bool) -> (String, bool) {
    let (body, blocked) = body(html, cids, allow_remote);
    (wrap(&body, ""), blocked)
}

/// The sanitized body fragment, and whether any remote resource was removed.
pub fn body(html: &str, cids: &HashMap<String, String>, allow_remote: bool) -> (String, bool) {
    let blocked = Arc::new(AtomicBool::new(false));
    let body = clean(html, cids, allow_remote, blocked.clone());
    let (body, css_blocked) = neutralize_style_blocks(&body, allow_remote);
    (body, blocked.load(Ordering::Relaxed) || css_blocked)
}

/// A standalone document around an already-sanitized body; `head` is trusted extra markup.
pub fn wrap(body: &str, head: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><base target=\"_blank\">\
         <style>{FRAME_STYLE}</style>{head}</head><body>{body}</body></html>"
    )
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn is_remote(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    u.starts_with("http:") || u.starts_with("https:") || u.starts_with("//")
}

fn clean(
    html: &str,
    cids: &HashMap<String, String>,
    allow_remote: bool,
    blocked: Arc<AtomicBool>,
) -> String {
    let cids = cids.clone();
    let mut b = ammonia::Builder::default();
    b.rm_clean_content_tags(&["style"])
        .add_tags(&["style", "center", "font"])
        .add_generic_attributes(&[
            "style",
            "class",
            "align",
            "valign",
            "bgcolor",
            "width",
            "height",
            "border",
            "cellpadding",
            "cellspacing",
            "color",
            "dir",
        ])
        .add_tag_attributes("font", &["face", "size", "color"])
        .url_schemes(HashSet::from(["http", "https", "mailto", "data", "cid"]))
        .link_rel(Some("noopener noreferrer"))
        .set_tag_attribute_value("a", "target", "_blank")
        .attribute_filter(
            move |element, attribute, value| match (element, attribute) {
                ("img", "src") => {
                    let v = value.trim();
                    if let Some(cid) = v.strip_prefix("cid:") {
                        return cids.get(cid).map(|d| Cow::Owned(d.clone()));
                    }
                    if v.to_ascii_lowercase().starts_with("data:image/") {
                        return Some(Cow::Borrowed(value));
                    }
                    if is_remote(v) {
                        if allow_remote {
                            return Some(Cow::Borrowed(value));
                        }
                        blocked.store(true, Ordering::Relaxed);
                    }
                    None
                }
                ("a", "href") => {
                    let v = value.trim().to_ascii_lowercase();
                    (v.starts_with("http:") || v.starts_with("https:") || v.starts_with("mailto:"))
                        .then_some(Cow::Borrowed(value))
                }
                (_, "style") => {
                    let (css, hit) = neutralize_css(value, allow_remote);
                    if hit {
                        blocked.store(true, Ordering::Relaxed);
                    }
                    Some(Cow::Owned(css))
                }
                _ => Some(Cow::Borrowed(value)),
            },
        );
    b.clean(html).to_string()
}

/// Strips `@import` always, and `url(...)` pointing anywhere but `data:` unless remote
/// content is allowed. Returns whether anything remote was removed.
pub fn neutralize_css(css: &str, allow_remote: bool) -> (String, bool) {
    let mut out = String::with_capacity(css.len());
    let mut hit = false;
    let lower = css.to_ascii_lowercase();
    let mut i = 0;
    while i < css.len() {
        let rest = &lower[i..];
        if rest.starts_with("@import") {
            hit = true;
            i += "@import".len();
            out.push_str("/* */");
            continue;
        }
        if rest.starts_with("expression(") || rest.starts_with("behavior:") {
            i += 1;
            out.push('_');
            continue;
        }
        if rest.starts_with("url(") {
            let close = rest.find(')').map_or(css.len(), |c| i + c + 1);
            let inner = lower[i + 4..close.saturating_sub(1).max(i + 4)]
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .trim();
            if inner.starts_with("data:") || allow_remote {
                out.push_str(&css[i..close]);
            } else {
                hit = true;
                out.push_str("none");
            }
            i = close;
            continue;
        }
        let ch = css[i..].chars().next().expect("in bounds");
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, hit)
}

fn neutralize_style_blocks(html: &str, allow_remote: bool) -> (String, bool) {
    let mut out = String::with_capacity(html.len());
    let mut hit = false;
    let mut rest = html;
    while let Some(start) = rest.find("<style") {
        let Some(open_end) = rest[start..].find('>').map(|e| start + e + 1) else {
            break;
        };
        let close = rest[open_end..]
            .find("</style>")
            .map_or(rest.len(), |c| open_end + c);
        out.push_str(&rest[..open_end]);
        let (css, h) = neutralize_css(&rest[open_end..close], allow_remote);
        hit |= h;
        out.push_str(&css);
        rest = &rest[close..];
        if rest.is_empty() {
            break;
        }
        out.push_str("</style>");
        rest = &rest["</style>".len()..];
    }
    out.push_str(rest);
    (out, hit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(html: &str) -> (String, bool) {
        document(html, &HashMap::new(), false)
    }

    #[test]
    fn strips_active_content() {
        let (d, _) = doc(
            r#"<script>alert(1)</script><img src=x onerror="alert(1)"><a href="javascript:alert(1)">x</a>
               <iframe src="https://evil"></iframe><form action="https://evil"><input></form>
               <svg><script>alert(1)</script></svg><object data="x"></object>"#,
        );
        for bad in [
            "<script",
            "onerror",
            "javascript:",
            "<iframe",
            "<form",
            "<input",
            "<object",
            "alert(1)",
        ] {
            assert!(!d.contains(bad), "{bad} survived: {d}");
        }
    }

    #[test]
    fn keeps_layout_and_safe_links() {
        let (d, _) = doc(
            r##"<table width="100%" bgcolor="#eee"><tr><td align="center"><a href="https://ok.example">ok</a></td></tr></table>"##,
        );
        assert!(d.contains(r#"href="https://ok.example""#));
        assert!(d.contains(r#"target="_blank""#));
        assert!(d.contains("noopener"));
        assert!(d.contains(r##"bgcolor="#eee""##));
    }

    #[test]
    fn blocks_remote_css_and_images() {
        let (d, blocked) = doc(
            r#"<style>@import url(https://t/x.css); .a{background:url('https://t/bg.png')} .b{background:url(data:image/png;base64,AA==)}</style>
               <div style="background-image: url(https://t/p.gif)">x</div><img src="https://t/p.gif">"#,
        );
        assert!(blocked);
        assert!(!d.contains("https://t/"), "{d}");
        assert!(d.contains("data:image/png;base64,AA=="));
    }

    #[test]
    fn allows_remote_when_asked() {
        let (d, blocked) = document(r#"<img src="https://t/p.gif">"#, &HashMap::new(), true);
        assert!(!blocked);
        assert!(d.contains("https://t/p.gif"));
    }

    #[test]
    fn css_scanner_handles_unicode_and_unclosed() {
        assert_eq!(
            neutralize_css("a{content:'é'} url(https://x", false).0,
            "a{content:'é'} none"
        );
        assert_eq!(
            neutralize_css("color:red", false),
            ("color:red".to_owned(), false)
        );
    }
}
