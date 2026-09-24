#!/usr/bin/env python3
"""
mdconvert.py
A small, purpose-built Markdown -> HTML converter for the MSX docs site.

Not a general CommonMark implementation — handles exactly the constructs
README.md and docs/format-spec.md actually use (surveyed directly before
writing this, not assumed): ATX headers (#..####), fenced code blocks
(```lang ... ```, language becomes a CSS class for optional
syntax-highlight hooks), GFM pipe tables, unordered lists, horizontal
rules (---), inline code (`x`), bold (**x**), and paragraphs. No nested
lists, no blockquotes, no inline links — none appear in either source
file. Deliberately no external dependency (no `pip install markdown` in
CI) since this is small enough to own outright and test directly.

Headers get a stable `id` (slugified text) so docs pages can be deep-
linked and get an auto-generated in-page table of contents.
"""

import html
import re


def _slugify(text: str) -> str:
    s = re.sub(r"[^\w\s-]", "", text.lower())
    s = re.sub(r"[\s_]+", "-", s).strip("-")
    return s or "section"


def _inline(text: str) -> str:
    """Inline-level formatting: escape HTML first, then apply inline code,
    bold, and italic — in that order, so markup characters inside a code
    span are never reinterpreted as bold/italic markers."""
    text = html.escape(text, quote=False)

    # Inline code spans first — protects their content from bold/italic
    # substitution below by pulling it out and re-inserting after.
    spans = []

    def stash_code(m):
        spans.append(m.group(1))
        return f"\x00{len(spans) - 1}\x00"

    text = re.sub(r"`([^`]+)`", stash_code, text)

    text = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", text)
    text = re.sub(r"(?<!\*)\*([^*]+)\*(?!\*)", r"<em>\1</em>", text)

    def restore_code(m):
        return f'<code>{spans[int(m.group(1))]}</code>'

    text = re.sub(r"\x00(\d+)\x00", restore_code, text)
    return text


def convert(md: str) -> tuple[str, list[dict]]:
    """Returns (body_html, toc) where toc is a list of
    {level, id, text} for every h2/h3 heading (h1 is the page title,
    left out of its own page's table of contents)."""
    lines = md.replace("\r\n", "\n").split("\n")
    out = []
    toc = []
    i = 0
    n = len(lines)

    def flush_list(buf):
        if buf:
            out.append("<ul>")
            for item in buf:
                out.append(f"<li>{_inline(item)}</li>")
            out.append("</ul>")
            buf.clear()

    def flush_para(buf):
        if buf:
            out.append(f"<p>{_inline(' '.join(buf))}</p>")
            buf.clear()

    list_buf: list[str] = []
    para_buf: list[str] = []

    while i < n:
        line = lines[i]

        # Fenced code block
        m = re.match(r"^```(\w*)\s*$", line)
        if m:
            flush_list(list_buf)
            flush_para(para_buf)
            lang = m.group(1) or "text"
            i += 1
            code_lines = []
            while i < n and not re.match(r"^```\s*$", lines[i]):
                code_lines.append(lines[i])
                i += 1
            i += 1  # skip closing fence
            code = html.escape("\n".join(code_lines))
            out.append(f'<pre class="code lang-{lang}"><code>{code}</code></pre>')
            continue

        # ATX heading
        m = re.match(r"^(#{1,4})\s+(.*)$", line)
        if m:
            flush_list(list_buf)
            flush_para(para_buf)
            level = len(m.group(1))
            text = m.group(2).strip()
            # Strip a trailing "`tag`" backtick-quoted anchor some
            # headings use (e.g. "### `rect`") down to plain text for the
            # slug, but keep backticks in the rendered heading itself via
            # _inline.
            slug = _slugify(re.sub(r"`", "", text))
            out.append(f'<h{level} id="{slug}">{_inline(text)}</h{level}>')
            if level in (2, 3):
                toc.append({"level": level, "id": slug, "text": text})
            i += 1
            continue

        # Horizontal rule
        if re.match(r"^-{3,}\s*$", line):
            flush_list(list_buf)
            flush_para(para_buf)
            out.append("<hr>")
            i += 1
            continue

        # Pipe table (header row, separator row, body rows)
        if "|" in line and i + 1 < n and re.match(r"^\s*\|?[\s:|-]+\|[\s:|-]*$", lines[i + 1]):
            flush_list(list_buf)
            flush_para(para_buf)
            header_cells = [c.strip() for c in line.strip().strip("|").split("|")]
            out.append('<table class="doc-table"><thead><tr>')
            for c in header_cells:
                out.append(f"<th>{_inline(c)}</th>")
            out.append("</tr></thead><tbody>")
            i += 2
            while i < n and "|" in lines[i]:
                row_cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                out.append("<tr>")
                for c in row_cells:
                    out.append(f"<td>{_inline(c)}</td>")
                out.append("</tr>")
                i += 1
            out.append("</tbody></table>")
            continue

        # Unordered list item
        m = re.match(r"^[-*]\s+(.*)$", line)
        if m:
            flush_para(para_buf)
            list_buf.append(m.group(1))
            i += 1
            continue

        # Blank line: paragraph/list break
        if line.strip() == "":
            flush_list(list_buf)
            flush_para(para_buf)
            i += 1
            continue

        # Plain text -> accumulate into current paragraph
        flush_list(list_buf)
        para_buf.append(line.strip())
        i += 1

    flush_list(list_buf)
    flush_para(para_buf)

    return "\n".join(out), toc


if __name__ == "__main__":
    import sys
    src = open(sys.argv[1]).read()
    body, toc = convert(src)
    print(body)
    print("\n<!-- TOC:", toc, "-->", file=sys.stderr)
