//! The web engine behind `browse` (terminal) and `web` (window): a
//! forgiving [`html`] parser and a text [`layout`] with links and forms.
//! There is no CSS or JavaScript: pages are shown as structured text, the
//! way Lynx or w3m show them.

#![no_std]

extern crate alloc;

pub mod html;
pub mod layout;

pub use layout::{layout, plain, Page};

/// Parses and lays out an HTML document.
pub fn render(html: &str, width: usize) -> Page {
    layout::layout(&html::parse(html), width)
}

/// Guesses whether a body is HTML (when the server does not say).
pub fn looks_like_html(body: &[u8]) -> bool {
    let start: alloc::string::String = core::str::from_utf8(&body[..body.len().min(512)]).unwrap_or("").trim_start().chars().take(64).collect::<alloc::string::String>().to_ascii_lowercase();
    start.starts_with("<!doctype html") || start.starts_with("<html") || start.starts_with("<head") || start.starts_with("<body") || start.starts_with("<!--")
}

#[cfg(test)]
mod tests;
