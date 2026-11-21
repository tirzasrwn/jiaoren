//! Detection of the HTML placeholders the Markdown render hook replaces:
//! `<mdpara id="…">`, `<mdtable id="…">`, `<mdlist id="…">`, `<mdheading id="…">`,
//! `<mdcode id="…">`, `<mdimg id="…">` and `<mermaid id="…">`.

/// One placeholder found in the hook's HTML, holding the referenced id.
pub(crate) enum Placeholder<'a> {
    Mermaid(&'a str),
    List(&'a str),
    Table(&'a str),
    Paragraph(&'a str),
    Image(&'a str),
    Heading(&'a str),
    Code(&'a str),
}

/// Parse `html` as exactly one placeholder. Checked in hook dispatch order:
/// paragraph, table, list, heading, code, image, mermaid. Whitespace around the
/// tag is ignored; trailing attributes and empty ids are rejected.
pub(crate) fn parse(html: &str) -> Option<Placeholder<'_>> {
    parse_kind(html, "<mdpara")
        .map(Placeholder::Paragraph)
        .or_else(|| parse_kind(html, "<mdtable").map(Placeholder::Table))
        .or_else(|| parse_kind(html, "<mdlist").map(Placeholder::List))
        .or_else(|| parse_kind(html, "<mdheading").map(Placeholder::Heading))
        .or_else(|| parse_kind(html, "<mdcode").map(Placeholder::Code))
        .or_else(|| parse_kind(html, "<mdimg").map(Placeholder::Image))
        .or_else(|| parse_kind(html, "<mermaid").map(Placeholder::Mermaid))
}

fn parse_kind<'a>(html: &'a str, prefix: &str) -> Option<&'a str> {
    let s = html.trim();
    let rest = s.strip_prefix(prefix)?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("id=")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let id = &rest[..end];
    if id.is_empty() {
        return None;
    }
    let after = &rest[end + 1..];
    let after = after.trim_end();
    if !after.ends_with('>') {
        return None;
    }
    if !after[..after.len() - 1].trim().is_empty() {
        return None;
    }
    Some(id)
}

#[cfg(test)]
mod tests {
    mod placeholder {
        use super::super::{parse, Placeholder};

        #[test]
        fn parses_placeholder_ids() {
            assert!(matches!(
                parse(r#"<mermaid id="abc123">"#),
                Some(Placeholder::Mermaid("abc123"))
            ));
            assert!(parse("<div>hi</div>").is_none());
            assert!(parse(r#"<mermaid id="">"#).is_none());
            // Trailing attributes and junk after the tag are rejected.
            assert!(parse(r#"<mermaid id="x" data-y="1">"#).is_none());
            assert!(parse(r#"<mermaidx id="x">"#).is_none());
            // Surrounding whitespace is ignored.
            assert!(matches!(
                parse("  <mermaid id=\"abc\">  \n"),
                Some(Placeholder::Mermaid("abc"))
            ));
        }

        #[test]
        fn parses_table_placeholder_ids() {
            assert!(matches!(
                parse(r#"<mdtable id="abc">"#),
                Some(Placeholder::Table("abc"))
            ));
            assert!(parse(r#"<mdtable id="">"#).is_none());
            assert!(
                parse(r#"<mermaid id="x">"#).is_some_and(|p| matches!(p, Placeholder::Mermaid(_)))
            );
            assert!(parse(r#"<mdtable id="x" class="t">"#).is_none());
            assert!(matches!(
                parse("\n<mdtable id=\"abc\">\t"),
                Some(Placeholder::Table("abc"))
            ));
        }

        #[test]
        fn parses_list_placeholder_ids() {
            assert!(matches!(
                parse(r#"<mdlist id="abc">"#),
                Some(Placeholder::List("abc"))
            ));
            assert!(parse(r#"<mdlist id="">"#).is_none());
            assert!(parse(r#"<mdlist id="x">"#).is_some_and(|p| matches!(p, Placeholder::List(_))));
            assert!(parse(r#"<mdlist id="x" extra>"#).is_none());
            assert!(matches!(
                parse(" <mdlist id=\"abc\"> "),
                Some(Placeholder::List("abc"))
            ));
        }

        #[test]
        fn parses_image_placeholder_ids() {
            assert!(matches!(
                parse(r#"<mdimg id="abc">"#),
                Some(Placeholder::Image("abc"))
            ));
            assert!(parse(r#"<mdimg id="">"#).is_none());
            assert!(parse(r#"<mdimg id="x" class="i">"#).is_none());
            assert!(matches!(
                parse(" <mdimg id=\"abc\"> "),
                Some(Placeholder::Image("abc"))
            ));
        }

        #[test]
        fn parses_paragraph_placeholder_ids() {
            assert!(matches!(
                parse(r#"<mdpara id="abc">"#),
                Some(Placeholder::Paragraph("abc"))
            ));
            assert!(parse(r#"<mdpara id="">"#).is_none());
            assert!(
                parse(r#"<mdtable id="x">"#).is_some_and(|p| matches!(p, Placeholder::Table(_)))
            );
            assert!(parse(r#"<mdpara id="x" oops>"#).is_none());
            assert!(parse("").is_none());
            assert!(matches!(
                parse(" <mdpara id=\"abc\"> "),
                Some(Placeholder::Paragraph("abc"))
            ));
        }
        #[test]
        fn parses_heading_and_code_placeholder_ids() {
            assert!(matches!(
                parse(r#"<mdheading id="h1">"#),
                Some(Placeholder::Heading("h1"))
            ));
            assert!(matches!(
                parse(" <mdcode id=\"c1\"> "),
                Some(Placeholder::Code("c1"))
            ));
            assert!(parse(r#"<mdheading id="">"#).is_none());
            assert!(parse(r#"<mdcode id="x" data-lang="rust">"#).is_none());
            // The longer tag never matches the shorter prefix.
            assert!(parse(r#"<mdcodex id="x">"#).is_none());
        }
    }
}
