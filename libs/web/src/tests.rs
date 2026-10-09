extern crate std;

use super::html::{self, decode_entities, Node};
use super::layout::*;
use super::*;
use alloc::string::String;
use alloc::vec::Vec;

const PAGE: &str = r#"<!DOCTYPE html>
<html><head><title>Test &amp; page</title><style>p { color: red }</style>
<script>var x = "<p>not text</p>";</script></head>
<body>
<h1>Main title</h1>
<p>Some <b>bold</b> and <a href="/next">a link</a>, then &copy; 2026&nbsp;Huldra.
<p>Second paragraph without a closing tag
<ul><li>one<li>two <a href="b.html">bee</a></ul>
<ol start="3"><li>third</li><li>fourth</li></ol>
<table><tr><th>Name</th><th>Size</th></tr><tr><td>kernel</td><td>12 KiB</td></tr></table>
<form action="/search" method="get"><input name="q" value="rust os"><input type="hidden" name="lang" value="en">
<input type="checkbox" name="exact" checked><select name="sort"><option value="new">Newest<option value="old" selected>Oldest</select>
<input type="submit" value="Go"></form>
<pre>  indented
    more</pre>
<noscript>shown without scripts</noscript>
<img src="x.png" alt="a picture"><hr>
<p id="end">The end.</p>
</body></html>"#;

fn texts(p: &Page) -> Vec<String> {
    (0..p.lines.len()).map(|i| p.line_text(i)).collect()
}

#[test]
fn entities() {
    assert_eq!(decode_entities("a &amp; b &lt;c&gt; &#169; &#x41; &unknown; &"), "a & b <c> © A &unknown; &");
    assert_eq!(decode_entities("&copy 2026"), "© 2026");
}

#[test]
fn parsing() {
    let doc = html::parse(PAGE);
    assert_eq!(doc.find("title").unwrap().text(), "Test & page");
    let body = doc.find("body").unwrap();
    // The two paragraphs are siblings, the list items too.
    let ps: Vec<_> = body.children.iter().filter(|n| matches!(n, Node::Element(e) if e.name == "p")).collect();
    assert_eq!(ps.len(), 3);
    let ul = body.find("ul").unwrap();
    assert_eq!(ul.children.iter().filter(|n| matches!(n, Node::Element(e) if e.name == "li")).count(), 2);
    // Script content is not text.
    assert!(!body.text().contains("not text"));
    let bad = html::parse("<p>a <b>b</i> c</p> d </div> <x");
    assert!(bad.text().contains("a b c d"));
}

#[test]
fn layout_of_a_page() {
    let p = render(PAGE, 80);
    let t = texts(&p);
    let all = t.join("\n");
    assert_eq!(p.title, "Test & page");
    assert_eq!(t[0], "Main title");
    assert!(t.contains(&"Some bold and a link, then © 2026 Huldra.".into()), "{all}");
    assert!(t.contains(&"Second paragraph without a closing tag".into()));
    assert!(t.contains(&"  • one".into()) && t.contains(&"  • two bee".into()), "{all}");
    assert!(t.contains(&"  3. third".into()) && t.contains(&"  4. fourth".into()), "{all}");
    assert!(t.contains(&"Name   │ Size".into()) && t.contains(&"kernel │ 12 KiB".into()), "{all}");
    assert!(all.contains("[rust os_________] [x] [Oldest ▾] [ Go ]"), "{all}");
    assert!(all.contains("  indented\n    more"), "{all}");
    assert!(all.contains("shown without scripts"));
    assert!(all.contains("[a picture]"));
    assert!(!all.contains("color: red"));
    assert_eq!(p.links, ["/next", "b.html"]);
    assert_eq!(p.link_line(0), Some(t.iter().position(|l| l.contains("a link")).unwrap()));
    // Styles.
    let title_span = &p.lines[0][1];
    assert!(title_span.style & HEADING != 0);
    let link_span = p.lines.iter().flatten().find(|s| s.link == Some(0)).unwrap();
    assert!(link_span.style & LINK != 0);
    // Anchors.
    let end = p.anchors.iter().find(|(id, _)| id == "end").unwrap().1;
    assert_eq!(p.line_text(end), "The end.");
}

#[test]
fn forms() {
    let mut p = render(PAGE, 80);
    let go = p.fields.iter().position(|f| f.kind == FieldKind::Submit).unwrap();
    let (method, action, data) = p.submission(0, Some(go));
    assert_eq!((method.as_str(), action.as_str()), ("get", "/search"));
    assert_eq!(data, "q=rust+os&lang=en&exact=on&sort=old");
    p.fields[0].value = "a&b".into();
    p.fields[2].checked = false;
    assert_eq!(p.submission(0, None).2, "q=a%26b&lang=en&sort=old");
}

#[test]
fn wrapping() {
    let long = alloc::format!("<p>{}</p>", "word ".repeat(100));
    let p = render(&long, 40);
    assert!(p.lines.len() > 10);
    for i in 0..p.lines.len() {
        assert!(p.line_text(i).chars().count() <= 40, "{:?}", p.line_text(i));
    }
    let url = alloc::format!("<p>{}</p>", "x".repeat(130));
    let p = render(&url, 40);
    assert_eq!(p.lines.len(), 4);
    let nested = render("<ul><li>a<ul><li>b</li></ul></li></ul>", 40);
    assert_eq!(texts(&nested), ["  • a", "    ◦ b"]);
}

#[test]
fn tables_like_hacker_news() {
    let html = r#"<table><tr><td align="right">1.</td><td><a href="vote"><div class="votearrow"></div></a></td>
<td><a href="https://example.com/story">A rather long story title that will not fit next to the rank on a narrow screen</a> (example.com)</td></tr>
<tr><td colspan="2"></td><td>100 points by someone</td></tr></table>"#;
    let p = render(html, 40);
    let t = texts(&p);
    // Rank and title side by side, the title wrapping in its column; the
    // empty vote column is gone.
    assert_eq!(t[0], "1. │ A rather long story title that will", "{t:?}");
    assert!(t[1].starts_with("   │ not fit next"), "{t:?}");
    assert!(t.iter().all(|l| l.chars().count() <= 40));
    // The subtext row's colspan=2 covers rank and vote: it lines up with titles.
    assert!(t.iter().any(|l| l == "   │ 100 points by someone"), "{t:?}");
    // Links inside cells are recorded once (the empty vote link went with its column).
    assert_eq!(p.links, ["https://example.com/story"]);
}

#[test]
fn plain_text_and_detection() {
    assert!(looks_like_html(b"  <!DOCTYPE html><p>"));
    assert!(!looks_like_html(b"just text"));
    let p = plain("a\tb\n\nc", 80);
    assert_eq!(texts(&p), ["a    b", "", "c"]);
}
