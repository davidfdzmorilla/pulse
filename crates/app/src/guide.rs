//! Ayuda → Guía de Pulse: everything the app does, by area and topic, on a page
//! of its own over the whole window.
//!
//! The text is `docs/guia.md`, built into the app, in a small Markdown:
//! - `# Área` groups topics in the side bar; `## Tema` is a page, and its first
//!   paragraph is the lead; `### Apartado` goes in the page's index, `####` is a
//!   smaller heading.
//! - `1. ` steps, `- ` points (two spaces deeper per level), `| a | b |` tables.
//! - `> Consejo: …`, `> Nota: …`, `> Atención: …` callouts.
//! - `+++ Título` … `+++` a block folded away until clicked.
//! - Inside lines: **bold**, `code`, `{⌘T}` keys (written as this system writes
//!   them), `[[Tema]]`, `[[Tema|texto]]`, `[[Tema#Apartado]]` and `[[#Apartado]]` links.

use crate::app::PulseApp;
use crate::theme::{self, ACCENT, TEXT, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, text::LayoutJob, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, TextFormat};

const TEXT_MD: &str = include_str!("../../../docs/guia.md");

#[derive(Clone, Copy, Debug, PartialEq)]
enum Callout {
    Tip,
    Note,
    Warning,
}

#[derive(Clone, Debug, PartialEq)]
enum Block {
    Heading(String),
    Minor(String),
    Paragraph(String),
    /// A point and how deep it is (0: top level).
    Point(usize, String),
    /// A numbered step.
    Step(usize, String),
    Callout(Callout, String),
    Table(Vec<Vec<String>>),
    /// Folded away under its title until clicked.
    Fold(String, Vec<Block>),
}

#[derive(Clone, Debug)]
struct Section {
    area: String,
    title: String,
    /// The first paragraph: what the topic is for.
    lead: String,
    blocks: Vec<Block>,
    /// All of it folded (see `fold`), without marks, for the search.
    folded: String,
}

/// The page's state: where you are, where you were, what you look for.
#[derive(Default)]
pub struct Guide {
    pub open: bool,
    search: String,
    /// The search the topic was chosen for (a new search picks its best topic).
    searched: String,
    section: usize,
    sections: Vec<Section>,
    back: Vec<usize>,
    forward: Vec<usize>,
    /// A heading to bring into view (from the index or a link), once laid out.
    scroll_to: Option<String>,
    /// Scroll back to the top on the next frame (a new topic).
    to_top: bool,
}

impl Guide {
    fn sections(&mut self) -> &[Section] {
        if self.sections.is_empty() {
            self.sections = parse(TEXT_MD);
        }
        &self.sections
    }

    /// Goes to a topic, remembering this one for ‹.
    fn go(&mut self, section: usize) {
        if section != self.section {
            self.back.push(self.section);
            self.forward.clear();
            self.section = section;
        }
        self.to_top = true;
        self.scroll_to = None;
    }

    /// Follows a `[[Tema#Apartado]]` link (`#Apartado` alone: this topic).
    fn follow(&mut self, target: &str) {
        let (title, heading) = target.split_once('#').map_or((target, None), |(t, h)| (t, Some(h)));
        let section = if title.is_empty() { Some(self.section) } else { self.sections.iter().position(|s| s.title == title) };
        if let Some(section) = section {
            self.go(section);
            self.scroll_to = heading.map(str::to_string);
        }
    }

    /// Opens the guide at a topic (by its title).
    #[allow(dead_code)]
    pub fn open_at(&mut self, title: &str) {
        self.sections();
        self.open = true;
        self.search.clear();
        self.searched.clear();
        self.follow(title);
    }
}

fn parse(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut area = String::new();
    let mut paragraph = String::new();
    // A `+++` block being read: its title and what is in it.
    let mut fold: Option<(String, Vec<Block>)> = None;
    // Whether the line before was part of a callout (`> `).
    let mut in_callout = false;
    fn flush(sections: &mut [Section], fold: &mut Option<(String, Vec<Block>)>, paragraph: &mut String) {
        if !paragraph.trim().is_empty() {
            push(sections, fold, Block::Paragraph(paragraph.trim().to_string()));
        }
        paragraph.clear();
    }
    fn push(sections: &mut [Section], fold: &mut Option<(String, Vec<Block>)>, block: Block) {
        match (fold, sections.last_mut()) {
            (Some((_, blocks)), _) => blocks.push(block),
            (None, Some(section)) => section.blocks.push(block),
            _ => {}
        }
    }
    fn last<'a>(sections: &'a mut [Section], fold: &'a mut Option<(String, Vec<Block>)>) -> Option<&'a mut Block> {
        match fold {
            Some((_, blocks)) => blocks.last_mut(),
            None => sections.last_mut().and_then(|s| s.blocks.last_mut()),
        }
    }
    for line in text.lines() {
        let trimmed = line.trim_start();
        let callout_line = trimmed.strip_prefix("> ").or((trimmed == ">").then_some(""));
        let was_callout = std::mem::replace(&mut in_callout, callout_line.is_some());
        if let Some(name) = line.strip_prefix("# ") {
            flush(&mut sections, &mut fold, &mut paragraph);
            area = name.trim().to_string();
            continue;
        }
        if let Some(title) = line.strip_prefix("## ") {
            flush(&mut sections, &mut fold, &mut paragraph);
            if let Some((title, blocks)) = fold.take() {
                push(&mut sections, &mut None, Block::Fold(title, blocks));
            }
            sections.push(Section { area: area.clone(), title: title.trim().to_string(), lead: String::new(), blocks: Vec::new(), folded: String::new() });
            continue;
        }
        // Anything before the first topic (the file's own notes).
        if sections.is_empty() {
            continue;
        }
        if let Some(title) = trimmed.strip_prefix("+++") {
            flush(&mut sections, &mut fold, &mut paragraph);
            if let Some((title, blocks)) = fold.take() {
                push(&mut sections, &mut None, Block::Fold(title, blocks));
            }
            if !title.trim().is_empty() {
                fold = Some((title.trim().to_string(), Vec::new()));
            }
        } else if let Some(heading) = line.strip_prefix("#### ") {
            flush(&mut sections, &mut fold, &mut paragraph);
            push(&mut sections, &mut fold, Block::Minor(heading.trim().to_string()));
        } else if let Some(heading) = line.strip_prefix("### ") {
            flush(&mut sections, &mut fold, &mut paragraph);
            if let Some((title, blocks)) = fold.take() {
                push(&mut sections, &mut None, Block::Fold(title, blocks));
            }
            push(&mut sections, &mut fold, Block::Heading(heading.trim().to_string()));
        } else if let Some(said) = callout_line {
            flush(&mut sections, &mut fold, &mut paragraph);
            let continues = was_callout && matches!(last(&mut sections, &mut fold), Some(Block::Callout(..)));
            if continues {
                if let Some(Block::Callout(_, text)) = last(&mut sections, &mut fold) {
                    if said.is_empty() {
                        text.push('\n');
                    } else {
                        if !text.ends_with('\n') {
                            text.push(' ');
                        }
                        text.push_str(said.trim());
                    }
                }
            } else {
                let kinds = [("Consejo:", Callout::Tip), ("Nota:", Callout::Note), ("Atención:", Callout::Warning)];
                let (kind, said) = kinds.iter().find_map(|(label, kind)| said.strip_prefix(label).map(|rest| (*kind, rest))).unwrap_or((Callout::Note, said));
                let mut said: Vec<char> = said.trim().chars().collect();
                if let Some(first) = said.first_mut() {
                    *first = first.to_uppercase().next().unwrap_or(*first);
                }
                push(&mut sections, &mut fold, Block::Callout(kind, said.into_iter().collect()));
            }
        } else if let Some(point) = trimmed.strip_prefix("- ") {
            flush(&mut sections, &mut fold, &mut paragraph);
            let depth = (line.len() - trimmed.len()) / 2;
            push(&mut sections, &mut fold, Block::Point(depth, point.trim().to_string()));
        } else if let Some((number, step)) = trimmed.split_once(". ").filter(|(n, _)| !n.is_empty() && n.len() <= 2 && n.chars().all(|c| c.is_ascii_digit()) && line == trimmed) {
            flush(&mut sections, &mut fold, &mut paragraph);
            push(&mut sections, &mut fold, Block::Step(number.parse().unwrap_or(1), step.trim().to_string()));
        } else if trimmed.starts_with('|') {
            flush(&mut sections, &mut fold, &mut paragraph);
            let cells: Vec<String> = trimmed.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect();
            // The |---|---| line under the header.
            if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')) {
                continue;
            }
            match last(&mut sections, &mut fold) {
                Some(Block::Table(rows)) => rows.push(cells),
                _ => push(&mut sections, &mut fold, Block::Table(vec![cells])),
            }
        } else if trimmed.is_empty() {
            flush(&mut sections, &mut fold, &mut paragraph);
        } else if let Some(Block::Point(_, text) | Block::Step(_, text)) = last(&mut sections, &mut fold).filter(|_| paragraph.is_empty() && line.starts_with("  ")) {
            // A point or step that goes on on the next line.
            text.push(' ');
            text.push_str(trimmed);
        } else {
            paragraph.push_str(trimmed);
            paragraph.push(' ');
        }
    }
    flush(&mut sections, &mut fold, &mut paragraph);
    if let Some((title, blocks)) = fold.take() {
        push(&mut sections, &mut None, Block::Fold(title, blocks));
    }
    for section in &mut sections {
        if let Some(Block::Paragraph(_)) = section.blocks.first() {
            let Block::Paragraph(lead) = section.blocks.remove(0) else { unreachable!() };
            section.lead = lead;
        }
        let mut all = format!("{}\n{}", section.title, section.lead);
        fn gather(blocks: &[Block], all: &mut String) {
            for block in blocks {
                all.push('\n');
                match block {
                    Block::Heading(t) | Block::Minor(t) | Block::Paragraph(t) | Block::Point(_, t) | Block::Step(_, t) | Block::Callout(_, t) => all.push_str(t),
                    Block::Table(rows) => all.push_str(&rows.iter().map(|r| r.join(" ")).collect::<Vec<_>>().join("\n")),
                    Block::Fold(title, blocks) => {
                        all.push_str(title);
                        gather(blocks, all);
                    }
                }
            }
        }
        gather(&section.blocks, &mut all);
        section.folded = self::fold(&plain(&all));
    }
    sections
}

/// One character as the search compares it: lower case, without accents.
fn fold_char(c: char) -> char {
    match c.to_lowercase().next().unwrap_or(c) {
        'á' | 'à' | 'ä' | 'â' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        other => other,
    }
}

fn fold(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

/// The words looked for, folded.
fn words(search: &str) -> Vec<String> {
    search.split_whitespace().map(fold).collect()
}

/// The sections with every word of the search, best first: those whose title
/// has the words, then those that mention them most.
fn matches(sections: &[Section], words: &[String]) -> Vec<usize> {
    let mut found: Vec<(usize, usize)> = (0..sections.len())
        .filter(|&i| words.iter().all(|w| sections[i].folded.contains(w.as_str())))
        .map(|i| {
            let title = fold(&sections[i].title);
            let score = words.iter().map(|w| sections[i].folded.matches(w.as_str()).count() + if title.contains(w.as_str()) { 1000 } else { 0 }).sum();
            (i, score)
        })
        .collect();
    if !words.is_empty() {
        found.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    }
    found.into_iter().map(|(i, _)| i).collect()
}

/// A run of a line: its text and how it looks.
#[derive(Clone, Debug, Default, PartialEq)]
struct Run {
    text: String,
    bold: bool,
    code: bool,
    key: bool,
    link: Option<String>,
}

/// A line's runs: **bold**, `code`, {keys} and [[links]].
fn runs(text: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut current = Run::default();
    let mut rest = text;
    let flush = |current: &mut Run, runs: &mut Vec<Run>| {
        if !current.text.is_empty() {
            runs.push(current.clone());
        }
        current.text.clear();
    };
    while let Some(c) = rest.chars().next() {
        if !current.code {
            if let Some(after) = rest.strip_prefix("**") {
                flush(&mut current, &mut runs);
                current.bold = !current.bold;
                rest = after;
                continue;
            }
            if let Some((inside, after)) = rest.strip_prefix("[[").and_then(|r| r.split_once("]]")) {
                flush(&mut current, &mut runs);
                let (target, label) = inside.split_once('|').unwrap_or((inside, inside.trim_start_matches('#')));
                let label = if inside.contains('|') { label } else { target.rsplit('#').next().unwrap_or(target) };
                runs.push(Run { text: label.to_string(), bold: current.bold, link: Some(target.to_string()), ..Default::default() });
                rest = after;
                continue;
            }
            if let Some((inside, after)) = rest.strip_prefix('{').and_then(|r| r.split_once('}')) {
                flush(&mut current, &mut runs);
                runs.push(Run { text: theme::keys(inside), key: true, ..Default::default() });
                rest = after;
                continue;
            }
        }
        if c == '`' {
            flush(&mut current, &mut runs);
            current.code = !current.code;
        } else {
            current.text.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut current, &mut runs);
    runs
}

/// A line without its marks, as it reads.
fn plain(text: &str) -> String {
    text.lines().map(|line| runs(line).into_iter().map(|r| r.text).collect::<String>()).collect::<Vec<_>>().join("\n")
}

/// How a piece of text looks: its size, color and weight.
#[derive(Clone, Copy)]
struct Style {
    size: f32,
    color: Color32,
    bold: bool,
    /// Line height over the size.
    leading: f32,
}

impl Style {
    fn body() -> Self {
        Style { size: 14.0, color: TEXT, bold: false, leading: 1.5 }
    }
}

/// Something drawn behind or over part of a line.
#[derive(Clone, Debug, PartialEq)]
enum Mark {
    Key,
    Code,
    Link(String),
    Found,
}

/// A line laid out: the text, and which characters are keys, code, links or
/// words searched for.
fn line(text: &str, words: &[String], style: Style) -> (LayoutJob, Vec<(std::ops::Range<usize>, Mark)>) {
    let mut job = LayoutJob::default();
    let mut marks = Vec::new();
    let mut at = 0;
    for run in runs(text) {
        let mut format = TextFormat {
            font_id: if run.code {
                FontId::monospace(style.size - 1.5)
            } else if run.bold || style.bold {
                theme::bold(style.size)
            } else if run.key {
                theme::semibold(style.size - 2.0)
            } else {
                FontId::proportional(style.size)
            },
            color: if run.link.is_some() {
                ACCENT
            } else if run.bold && !style.bold {
                Color32::WHITE
            } else if run.code {
                Color32::from_rgb(0xf0, 0xc0, 0x80)
            } else {
                style.color
            },
            line_height: Some((style.size * style.leading).round()),
            valign: egui::Align::Center,
            ..Default::default()
        };
        // Keys and code get a little room for their box.
        let text = if run.key || run.code { format!("\u{2009}{}\u{2009}", run.text) } else { run.text.clone() };
        if run.key {
            format.color = Color32::from_rgb(0xee, 0xee, 0xee);
        }
        let count = text.chars().count();
        if run.key {
            marks.push((at..at + count, Mark::Key));
        } else if run.code {
            marks.push((at..at + count, Mark::Code));
        }
        if let Some(target) = &run.link {
            marks.push((at..at + count, Mark::Link(target.clone())));
        }
        // Where the words searched for are, in characters.
        let folded: Vec<char> = text.chars().map(fold_char).collect();
        for word in words {
            let word: Vec<char> = word.chars().collect();
            if word.is_empty() || word.len() > folded.len() {
                continue;
            }
            let mut start = 0;
            while start + word.len() <= folded.len() {
                if folded[start..start + word.len()] == word[..] {
                    marks.push((at + start..at + start + word.len(), Mark::Found));
                    start += word.len();
                } else {
                    start += 1;
                }
            }
        }
        job.append(&text, 0.0, format);
        at += count;
    }
    (job, marks)
}

/// The rectangles a range of characters covers, one per row.
fn char_rects(galley: &egui::Galley, origin: egui::Pos2, range: &std::ops::Range<usize>) -> Vec<Rect> {
    let mut rects: Vec<Rect> = Vec::new();
    let mut index = 0;
    for row in &galley.rows {
        let mut on_row: Option<Rect> = None;
        for glyph in &row.glyphs {
            if range.contains(&index) {
                let r = Rect::from_min_size(origin + row.pos.to_vec2() + vec2(glyph.pos.x, 0.0), vec2(glyph.advance_width, row.size.y));
                on_row = Some(on_row.map_or(r, |o| o.union(r)));
            }
            index += 1;
        }
        if row.ends_with_newline {
            index += 1;
        }
        rects.extend(on_row);
    }
    rects
}

/// Draws a line of the guide that wraps at `width`, with its keys, code, links
/// and search marks. Returns the link clicked, if any.
fn rich(ui: &mut egui::Ui, text: &str, words: &[String], style: Style, width: f32) -> Option<String> {
    let (mut job, marks) = line(text, words, style);
    job.wrap.max_width = width;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let sense = if marks.iter().any(|(_, m)| matches!(m, Mark::Link(_))) { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width.min(galley.size().x.max(1.0)), galley.size().y), sense);
    let painter = ui.painter();
    let hover = response.hover_pos();
    let mut clicked = None;
    for (range, mark) in &marks {
        for r in char_rects(&galley, rect.min, range) {
            // Glyphs sit a little above the row's middle.
            let middle = r.center().y - style.size * 0.17;
            let box_height = if *mark == Mark::Found { style.size * 1.3 } else { style.size * 1.45 };
            let r = Rect::from_x_y_ranges(r.x_range(), (middle - box_height / 2.0)..=(middle + box_height / 2.0));
            match mark {
                Mark::Key => {
                    let shadow = r.translate(vec2(0.0, 1.0));
                    painter.rect_filled(shadow, CornerRadius::same(4), Color32::from_rgb(0x12, 0x12, 0x12));
                    painter.rect(r, CornerRadius::same(4), Color32::from_rgb(0x3a, 0x3a, 0x3a), Stroke::new(1.0, Color32::from_rgb(0x50, 0x50, 0x50)), egui::StrokeKind::Inside);
                }
                Mark::Code => {
                    painter.rect_filled(r, CornerRadius::same(3), Color32::from_rgb(0x2a, 0x26, 0x20));
                }
                Mark::Found => {
                    painter.rect_filled(r.shrink2(vec2(0.0, 1.0)), CornerRadius::same(2), ACCENT.gamma_multiply(0.38));
                }
                Mark::Link(target) => {
                    let over = hover.is_some_and(|p| r.contains(p));
                    if over {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        if response.clicked() {
                            clicked = Some(target.clone());
                        }
                    }
                    let base = r.center().y + style.size * 0.55;
                    let color = if over { ACCENT } else { ACCENT.gamma_multiply(0.35) };
                    painter.hline(r.x_range(), base, Stroke::new(1.0, color));
                }
            }
        }
    }
    painter.galley(rect.min, galley, style.color);
    clicked
}

const CONTENT_WIDTH: f32 = 700.0;
const SIDEBAR_WIDTH: f32 = 248.0;
const INDEX_WIDTH: f32 = 210.0;
const SIDEBAR: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
const PAGE: Color32 = Color32::from_rgb(0x16, 0x16, 0x16);
const RULE: Color32 = Color32::from_rgb(0x2e, 0x2e, 0x2e);

impl Callout {
    fn look(self) -> (&'static str, Color32) {
        match self {
            Callout::Tip => ("Consejo", Color32::from_rgb(0x4c, 0xc9, 0xa8)),
            Callout::Note => ("Nota", Color32::from_rgb(0x7a, 0xa7, 0xe8)),
            Callout::Warning => ("Atención", ACCENT),
        }
    }
}

/// What a frame of the page asks for: a topic, a link, a heading's place.
#[derive(Default)]
struct Wanted {
    link: Option<String>,
    /// Each `###` heading and where it is, in the order drawn.
    headings: Vec<(String, Rect)>,
}

/// Draws a topic's blocks, `depth` folds deep.
fn blocks(ui: &mut egui::Ui, blocks: &[Block], words: &[String], width: f32, id: egui::Id, wanted: &mut Wanted) {
    let mut previous: Option<&Block> = None;
    for (index, block) in blocks.iter().enumerate() {
        // Space between blocks: more before a heading, less between points.
        let gap = match (previous, block) {
            (None, _) => 0.0,
            (_, Block::Heading(_)) => 30.0,
            (_, Block::Minor(_)) => 18.0,
            (Some(Block::Point(..)), Block::Point(..)) | (Some(Block::Step(..)), Block::Step(..)) => 6.0,
            (Some(Block::Heading(_) | Block::Minor(_)), _) => 8.0,
            _ => 14.0,
        };
        ui.add_space(gap);
        match block {
            Block::Heading(text) => {
                let top = ui.cursor().min;
                if let Some(link) = rich(ui, text, words, Style { size: 20.0, color: Color32::WHITE, bold: true, leading: 1.25 }, width) {
                    wanted.link = Some(link);
                }
                let rect = Rect::from_min_max(top, pos2(top.x + width, ui.cursor().min.y));
                ui.painter().hline(rect.x_range(), rect.max.y + 6.0, Stroke::new(1.0, RULE));
                ui.add_space(8.0);
                wanted.headings.push((plain(text), rect));
            }
            Block::Minor(text) => {
                wanted.link = rich(ui, text, words, Style { size: 15.5, color: Color32::WHITE, bold: true, leading: 1.3 }, width).or(wanted.link.take());
            }
            Block::Paragraph(text) => {
                wanted.link = rich(ui, text, words, Style::body(), width).or(wanted.link.take());
            }
            Block::Point(depth, text) => {
                let indent = 6.0 + *depth as f32 * 22.0;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(indent);
                    let (dot, _) = ui.allocate_exact_size(vec2(16.0, 21.0), Sense::hover());
                    let center = pos2(dot.left() + 3.0, dot.top() + 10.5);
                    if *depth == 0 {
                        ui.painter().circle_filled(center, 2.6, ACCENT.gamma_multiply(0.85));
                    } else {
                        ui.painter().circle_stroke(center, 2.4, Stroke::new(1.2, TEXT_DIM));
                    }
                    wanted.link = rich(ui, text, words, Style::body(), width - indent - 16.0).or(wanted.link.take());
                });
            }
            Block::Step(number, text) => {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let (badge, _) = ui.allocate_exact_size(vec2(32.0, 21.0), Sense::hover());
                    let center = pos2(badge.left() + 11.0, badge.top() + 10.5);
                    ui.painter().circle_filled(center, 11.0, Color32::from_rgb(0x3a, 0x2c, 0x16));
                    ui.painter().circle_stroke(center, 11.0, Stroke::new(1.0, ACCENT.gamma_multiply(0.6)));
                    ui.painter().text(center, Align2::CENTER_CENTER, number.to_string(), theme::bold(12.0), ACCENT);
                    wanted.link = rich(ui, text, words, Style::body(), width - 32.0).or(wanted.link.take());
                });
            }
            Block::Callout(kind, text) => {
                let (label, color) = kind.look();
                let frame = egui::Frame::new().fill(color.gamma_multiply(0.09)).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin { left: 16, right: 14, top: 11, bottom: 12 });
                let response = frame.show(ui, |ui| {
                    ui.set_width(width - 30.0);
                    ui.label(egui::RichText::new(label).font(theme::bold(12.5)).color(color));
                    ui.add_space(3.0);
                    for (i, paragraph) in text.split('\n').enumerate() {
                        if i > 0 {
                            ui.add_space(6.0);
                        }
                        wanted.link = rich(ui, paragraph, words, Style::body(), width - 30.0).or(wanted.link.take());
                    }
                });
                let r = response.response.rect;
                ui.painter().rect_filled(Rect::from_min_max(r.min, pos2(r.min.x + 3.0, r.max.y)), CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, color);
            }
            Block::Table(rows) => {
                if let Some(link) = table(ui, rows, words, width) {
                    wanted.link = Some(link);
                }
            }
            Block::Fold(title, inner) => {
                let fold_id = id.with(("fold", index));
                // A search that finds something inside opens it.
                let found = !words.is_empty() && {
                    let mut all = String::new();
                    for b in inner {
                        all.push_str(&format!("{b:?}"));
                    }
                    let all = fold(&plain(&all));
                    words.iter().all(|w| all.contains(w.as_str()))
                };
                let all_open = ui.data(|d| d.get_temp::<bool>(egui::Id::new("guide: every fold open"))).unwrap_or(false);
                let mut open = ui.data(|d| d.get_temp::<bool>(fold_id)).unwrap_or(false) || found || all_open;
                let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
                let fill = if response.hovered() { Color32::from_rgb(0x26, 0x26, 0x26) } else { Color32::from_rgb(0x1e, 0x1e, 0x1e) };
                let radius = if open { CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 } } else { CornerRadius::same(6) };
                ui.painter().rect(rect, radius, fill, Stroke::new(1.0, RULE), egui::StrokeKind::Inside);
                let arrow = pos2(rect.left() + 16.0, rect.center().y);
                let points = if open {
                    vec![arrow + vec2(-4.0, -2.0), arrow + vec2(4.0, -2.0), arrow + vec2(0.0, 3.0)]
                } else {
                    vec![arrow + vec2(-2.0, -4.0), arrow + vec2(3.0, 0.0), arrow + vec2(-2.0, 4.0)]
                };
                ui.painter().add(egui::Shape::convex_polygon(points, TEXT_DIM, Stroke::NONE));
                ui.painter().text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, title, theme::semibold(13.5), if response.hovered() { Color32::WHITE } else { TEXT });
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    open = !open;
                    ui.data_mut(|d| d.insert_temp(fold_id, open));
                }
                if open {
                    let frame = egui::Frame::new()
                        .fill(Color32::from_rgb(0x1a, 0x1a, 0x1a))
                        .stroke(Stroke::new(1.0, RULE))
                        .corner_radius(CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 })
                        .inner_margin(egui::Margin { left: 18, right: 18, top: 14, bottom: 16 });
                    ui.add_space(-ui.spacing().item_spacing.y - 1.0);
                    frame.show(ui, |ui| {
                        ui.set_width(width - 38.0);
                        let mut inside = Wanted::default();
                        self::blocks(ui, inner, words, width - 38.0, fold_id, &mut inside);
                        wanted.link = inside.link.or(wanted.link.take());
                    });
                }
            }
        }
        previous = Some(block);
    }
}

/// A table with its header, columns as wide as they need up to a share of `width`.
fn table(ui: &mut egui::Ui, rows: &[Vec<String>], words: &[String], width: f32) -> Option<String> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(1);
    let pad = 12.0;
    // How wide each column would like to be, on one line.
    let mut natural = vec![0.0f32; columns];
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let style = Style { bold: r == 0, ..Style::body() };
            let (job, _) = line(cell, words, Style { size: 13.5, ..style });
            let w = ui.fonts_mut(|f| f.layout_job(job)).size().x;
            natural[c] = natural[c].max(w + 2.0 * pad);
        }
    }
    // Each column gets what it needs if all fit; else the narrow ones keep theirs
    // and the rest share what is left.
    let mut widths = natural.clone();
    if natural.iter().sum::<f32>() > width {
        let mut fixed = vec![false; columns];
        loop {
            let left = width - (0..columns).filter(|&c| fixed[c]).map(|c| widths[c]).sum::<f32>();
            let open = (0..columns).filter(|&c| !fixed[c]).count().max(1);
            let share = left / open as f32;
            let mut changed = false;
            for c in 0..columns {
                if !fixed[c] && natural[c] <= share {
                    fixed[c] = true;
                    widths[c] = natural[c];
                    changed = true;
                }
            }
            if !changed {
                for c in 0..columns {
                    if !fixed[c] {
                        widths[c] = share;
                    }
                }
                break;
            }
        }
    } else if let Some(last) = widths.last_mut() {
        *last += width - natural.iter().sum::<f32>();
    }
    let mut clicked = None;
    let top = ui.cursor().min;
    let painter_index = ui.painter().add(egui::Shape::Noop);
    let mut stripes = Vec::new();
    ui.spacing_mut().item_spacing.y = 0.0;
    for (r, row) in rows.iter().enumerate() {
        let row_top = ui.cursor().min.y;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for c in 0..columns {
                let cell = row.get(c).map(String::as_str).unwrap_or("");
                let style = if r == 0 { Style { size: 12.5, color: TEXT_DIM, bold: true, leading: 1.4 } } else { Style { size: 13.5, ..Style::body() } };
                ui.vertical(|ui| {
                    ui.set_width(widths[c]);
                    ui.add_space(if r == 0 { 8.0 } else { 7.0 });
                    ui.horizontal_top(|ui| {
                        ui.add_space(pad);
                        if let Some(link) = rich(ui, cell, words, style, widths[c] - 2.0 * pad) {
                            clicked = Some(link);
                        }
                    });
                    ui.add_space(if r == 0 { 7.0 } else { 7.0 });
                });
            }
        });
        let row_bottom = ui.cursor().min.y;
        stripes.push((r, row_top, row_bottom));
    }
    ui.spacing_mut().item_spacing.y = 4.0;
    let bottom = ui.cursor().min.y;
    let outer = Rect::from_min_max(top, pos2(top.x + width, bottom));
    let mut shapes = vec![egui::Shape::rect_filled(outer, CornerRadius::same(6), Color32::from_rgb(0x1c, 0x1c, 0x1c))];
    for (r, row_top, row_bottom) in stripes {
        if r == 0 {
            let head = Rect::from_min_max(pos2(outer.left(), row_top), pos2(outer.right(), row_bottom));
            shapes.push(egui::Shape::rect_filled(head, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, Color32::from_rgb(0x24, 0x24, 0x24)));
        }
        if r > 0 {
            shapes.push(egui::Shape::hline(outer.x_range(), row_top, Stroke::new(1.0, RULE)));
        }
    }
    shapes.push(egui::Shape::rect_stroke(outer, CornerRadius::same(6), Stroke::new(1.0, RULE), egui::StrokeKind::Inside));
    ui.painter().set(painter_index, egui::Shape::Vec(shapes));
    clicked
}

/// The side bar: the search and the topics by area (or what the search found).
fn sidebar(ui: &mut egui::Ui, guide: &mut Guide, found: &[usize], words: &[String]) {
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.add_space(18.0);
        ui.label(egui::RichText::new("Guía de Pulse").font(theme::bold(17.0)).color(Color32::WHITE));
    });
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        let field = egui::TextEdit::singleline(&mut guide.search)
            .hint_text("Buscar en la guía")
            .font(FontId::proportional(13.5))
            .margin(vec2(10.0, 7.0))
            .background_color(Color32::from_rgb(0x2b, 0x2b, 0x2b))
            .desired_width(SIDEBAR_WIDTH - 28.0);
        let response = ui.add(field);
        crate::widgets::name(&response, egui::WidgetType::TextEdit, "Buscar en la guía");
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            response.request_focus();
        }
    });
    ui.add_space(10.0);
    egui::ScrollArea::vertical().id_salt("guide topics").auto_shrink(false).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let searching = !words.is_empty();
        if searching {
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                let said = match found.len() {
                    0 => "Nada encontrado".to_string(),
                    1 => "1 tema".to_string(),
                    n => format!("{n} temas"),
                };
                ui.label(egui::RichText::new(said).font(theme::small()).color(TEXT_DIM));
            });
            ui.add_space(6.0);
        }
        let mut area = "";
        let mut chosen = None;
        for &i in found {
            let section = &guide.sections[i];
            if !searching && section.area != area {
                area = &section.area;
                ui.add_space(if i == 0 { 4.0 } else { 14.0 });
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new(area.to_uppercase()).font(theme::semibold(10.5)).color(Color32::from_rgb(0x78, 0x78, 0x78)).extra_letter_spacing(0.8));
                });
                ui.add_space(3.0);
            }
            let selected = guide.section == i;
            let snippet = if searching { snippet(section, words) } else { None };
            let height = if snippet.is_some() { 48.0 } else { 27.0 };
            let (rect, response) = ui.allocate_exact_size(vec2(SIDEBAR_WIDTH, height), Sense::click());
            let row = rect.shrink2(vec2(8.0, 0.0));
            if selected {
                ui.painter().rect_filled(row, CornerRadius::same(5), Color32::from_rgb(0x33, 0x2a, 0x1c));
            } else if response.hovered() {
                ui.painter().rect_filled(row, CornerRadius::same(5), Color32::from_rgb(0x2a, 0x2a, 0x2a));
            }
            let title_y = if snippet.is_some() { row.top() + 15.0 } else { row.center().y };
            let color = if selected { ACCENT } else if response.hovered() { Color32::WHITE } else { TEXT };
            let font = if selected { theme::semibold(13.5) } else { FontId::proportional(13.5) };
            ui.painter().text(pos2(row.left() + 10.0, title_y), Align2::LEFT_CENTER, &section.title, font, color);
            if let Some(snippet) = snippet {
                let mut job = LayoutJob::single_section(snippet, TextFormat { font_id: FontId::proportional(11.5), color: TEXT_DIM, ..Default::default() });
                job.wrap.max_width = row.width() - 20.0;
                job.wrap.max_rows = 1;
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                ui.painter().galley(pos2(row.left() + 10.0, row.top() + 26.0), galley, TEXT_DIM);
            }
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                chosen = Some(i);
            }
        }
        if let Some(i) = chosen {
            guide.go(i);
        }
        ui.add_space(16.0);
    });
}

/// Where the search's words are in a topic: the sentence around the first.
fn snippet(section: &Section, words: &[String]) -> Option<String> {
    let text = plain(&format!("{}\n{}", section.lead, section.folded_source()));
    let folded: Vec<char> = text.chars().map(fold_char).collect();
    let word: Vec<char> = words.first()?.chars().collect();
    let at = (0..folded.len().saturating_sub(word.len() - 1)).find(|&i| folded[i..i + word.len()] == word[..])?;
    let chars: Vec<char> = text.chars().collect();
    let start = (0..at).rev().find(|&i| matches!(chars[i], '\n' | '.' | ':')).map_or(0, |i| i + 1);
    let start = start.max(at.saturating_sub(40));
    let out: String = chars[start..].iter().take_while(|&&c| c != '\n').take(120).collect();
    Some(format!("{}{}", if start > 0 && !matches!(chars[start - 1], '\n' | '.' | ':') { "…" } else { "" }, out.trim()))
}

impl Section {
    /// The topic's text as written (marks and all), for snippets.
    fn folded_source(&self) -> String {
        fn gather(blocks: &[Block], all: &mut String) {
            for block in blocks {
                all.push('\n');
                match block {
                    Block::Heading(t) | Block::Minor(t) | Block::Paragraph(t) | Block::Point(_, t) | Block::Step(_, t) | Block::Callout(_, t) => all.push_str(t),
                    Block::Table(rows) => all.push_str(&rows.iter().map(|r| r.join(" · ")).collect::<Vec<_>>().join("\n")),
                    Block::Fold(title, blocks) => {
                        all.push_str(title);
                        gather(blocks, all);
                    }
                }
            }
        }
        let mut all = String::new();
        gather(&self.blocks, &mut all);
        all
    }
}

/// A small round button with an arrow, for ‹ and ›.
fn arrow_button(ui: &mut egui::Ui, left: bool, enabled: bool, hint: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(0x2a, 0x2a, 0x2a));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let c = rect.center();
    let d = if left { -1.0 } else { 1.0 };
    let color = if enabled { TEXT } else { Color32::from_rgb(0x50, 0x50, 0x50) };
    ui.painter().line(vec![c + vec2(-2.5 * d, -5.0), c + vec2(2.5 * d, 0.0), c + vec2(-2.5 * d, 5.0)], Stroke::new(1.6, color));
    let response = response.on_hover_text(hint);
    crate::widgets::name(&response, egui::WidgetType::Button, hint);
    enabled && response.clicked()
}

/// The page: the side bar, the topic and its index.
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    let guide = &mut app.guide;
    if !guide.open {
        return;
    }
    guide.sections();
    let words = words(&guide.search);
    let found = matches(&guide.sections, &words);
    if guide.searched != guide.search {
        guide.searched.clone_from(&guide.search);
        if let Some(&first) = found.first().filter(|_| !words.is_empty()) {
            if first != guide.section {
                guide.go(first);
            }
        }
    }
    // Esc: first the search, then the guide.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        if guide.search.is_empty() {
            guide.open = false;
            return;
        }
        guide.search.clear();
    }
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("guide page")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.set_min_size(screen.size());
        ui.set_max_size(screen.size());
        let painter = ui.painter();
        painter.rect_filled(screen, CornerRadius::ZERO, PAGE);
        let side = Rect::from_min_size(screen.min, vec2(SIDEBAR_WIDTH, screen.height()));
        painter.rect_filled(side, CornerRadius::ZERO, SIDEBAR);
        painter.vline(side.right(), side.y_range(), Stroke::new(1.0, RULE));
        // Clicks don't reach what is under the page.
        let _ = ui.interact(screen, ui.id().with("cover"), Sense::click_and_drag());

        let mut side_ui = ui.new_child(egui::UiBuilder::new().max_rect(side));
        sidebar(&mut side_ui, guide, &found, &words);

        let main = Rect::from_min_max(pos2(side.right() + 1.0, screen.top()), screen.max);
        let show_index = main.width() >= CONTENT_WIDTH + INDEX_WIDTH + 120.0;
        let column_width = CONTENT_WIDTH.min(main.width() - 64.0).max(280.0);
        let index_room = if show_index { INDEX_WIDTH } else { 0.0 };
        let left = main.left() + ((main.width() - index_room - column_width) / 2.0).max(32.0);

        // The bar on top: back, forward, where you are, close.
        let bar = Rect::from_min_size(main.min, vec2(main.width(), 52.0));
        let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(16.0, 12.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        if arrow_button(&mut bar_ui, true, !guide.back.is_empty(), "Atrás") {
            if let Some(previous) = guide.back.pop() {
                guide.forward.push(guide.section);
                guide.section = previous;
                guide.to_top = true;
            }
        }
        if arrow_button(&mut bar_ui, false, !guide.forward.is_empty(), "Adelante") {
            if let Some(next) = guide.forward.pop() {
                guide.back.push(guide.section);
                guide.section = next;
                guide.to_top = true;
            }
        }
        if let Some(section) = guide.sections.get(guide.section) {
            bar_ui.add_space(10.0);
            bar_ui.label(egui::RichText::new(&section.area).font(FontId::proportional(13.0)).color(TEXT_DIM));
            bar_ui.label(egui::RichText::new("›").font(FontId::proportional(13.0)).color(Color32::from_rgb(0x5a, 0x5a, 0x5a)));
            bar_ui.label(egui::RichText::new(&section.title).font(FontId::proportional(13.0)).color(TEXT));
        }
        bar_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = ui.add(egui::Button::new(egui::RichText::new("Volver a Pulse").font(FontId::proportional(13.0))).min_size(vec2(0.0, 28.0)).corner_radius(CornerRadius::same(6)));
            let close = close.on_hover_text(theme::keys("Esc"));
            if close.clicked() {
                guide.open = false;
            }
        });
        ui.painter().hline(main.x_range(), bar.bottom(), Stroke::new(1.0, RULE));

        let body = Rect::from_min_max(pos2(main.left(), bar.bottom() + 1.0), main.max);
        let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
        let Some(section) = guide.sections.get(guide.section).cloned() else { return };
        let mut wanted = Wanted::default();
        let mut scroll = egui::ScrollArea::vertical().id_salt(("guide text", guide.section)).auto_shrink(false);
        if std::mem::take(&mut guide.to_top) {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        let scrolled = scroll.show(&mut body_ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.add_space(left - body.left());
                ui.vertical(|ui| {
                    ui.set_width(column_width);
                    ui.add_space(36.0);
                    ui.label(egui::RichText::new(section.area.to_uppercase()).font(theme::semibold(11.0)).color(ACCENT).extra_letter_spacing(0.9));
                    ui.add_space(6.0);
                    rich(ui, &section.title, &words, Style { size: 30.0, color: Color32::WHITE, bold: true, leading: 1.2 }, column_width);
                    if !section.lead.is_empty() {
                        ui.add_space(10.0);
                        wanted.link = rich(ui, &section.lead, &words, Style { size: 17.0, color: Color32::from_rgb(0xb4, 0xb4, 0xb4), bold: false, leading: 1.5 }, column_width);
                    }
                    ui.add_space(26.0);
                    blocks(ui, &section.blocks, &words, column_width, ui.id().with(guide.section), &mut wanted);
                    ui.add_space(40.0);
                    // The topics before and after.
                    let neighbours = [guide.section.checked_sub(1), Some(guide.section + 1).filter(|&n| n < guide.sections.len())];
                    ui.painter().hline(egui::Rangef::new(ui.cursor().left(), ui.cursor().left() + column_width), ui.cursor().top(), Stroke::new(1.0, RULE));
                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let card = (column_width - 12.0) / 2.0;
                        for (i, neighbour) in neighbours.into_iter().enumerate() {
                            let (rect, response) = ui.allocate_exact_size(vec2(card, 58.0), if neighbour.is_some() { Sense::click() } else { Sense::hover() });
                            let Some(n) = neighbour else { continue };
                            let fill = if response.hovered() { Color32::from_rgb(0x24, 0x24, 0x24) } else { Color32::from_rgb(0x1c, 0x1c, 0x1c) };
                            ui.painter().rect(rect, CornerRadius::same(6), fill, Stroke::new(1.0, RULE), egui::StrokeKind::Inside);
                            let (align, x) = if i == 0 { (Align2::LEFT_CENTER, rect.left() + 16.0) } else { (Align2::RIGHT_CENTER, rect.right() - 16.0) };
                            ui.painter().text(pos2(x, rect.top() + 19.0), align, if i == 0 { "‹ Anterior" } else { "Siguiente ›" }, FontId::proportional(11.5), TEXT_DIM);
                            ui.painter().text(pos2(x, rect.top() + 39.0), align, &guide.sections[n].title, theme::semibold(14.0), if response.hovered() { ACCENT } else { Color32::WHITE });
                            if response.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if response.clicked() {
                                wanted.link = Some(guide.sections[n].title.clone());
                            }
                        }
                    });
                    ui.add_space(48.0);
                });
            });
            // A heading asked for (index, link): bring it to the top.
            if let Some(heading) = guide.scroll_to.take() {
                match wanted.headings.iter().find(|(h, _)| *h == heading) {
                    Some((_, rect)) => ui.scroll_to_rect(rect.translate(vec2(0.0, -18.0)), Some(egui::Align::TOP)),
                    None => {}
                }
            }
        });

        // The page's index: its headings, the one you are reading lit.
        if show_index && !wanted.headings.is_empty() {
            let index = Rect::from_min_size(pos2(left + column_width + 48.0, body.top() + 40.0), vec2(INDEX_WIDTH - 40.0, body.height() - 40.0));
            let mut index_ui = ui.new_child(egui::UiBuilder::new().max_rect(index));
            index_ui.label(egui::RichText::new("EN ESTA PÁGINA").font(theme::semibold(10.5)).color(Color32::from_rgb(0x78, 0x78, 0x78)).extra_letter_spacing(0.8));
            index_ui.add_space(8.0);
            let reading_line = body.top() + 90.0;
            let current = wanted.headings.iter().rposition(|(_, r)| r.top() <= reading_line).unwrap_or(0);
            let at_bottom = scrolled.state.offset.y >= scrolled.content_size.y - scrolled.inner_rect.height() - 2.0;
            let current = if at_bottom && scrolled.state.offset.y > 0.0 { wanted.headings.len() - 1 } else { current };
            for (i, (heading, _)) in wanted.headings.iter().enumerate() {
                let mut job = LayoutJob::single_section(heading.clone(), TextFormat { font_id: if i == current { theme::semibold(12.5) } else { FontId::proportional(12.5) }, color: if i == current { Color32::WHITE } else { TEXT_DIM }, ..Default::default() });
                job.wrap.max_width = index.width() - 14.0;
                let galley = index_ui.fonts_mut(|f| f.layout_job(job));
                let (rect, response) = index_ui.allocate_exact_size(vec2(index.width(), galley.size().y + 8.0), Sense::click());
                if i == current {
                    index_ui.painter().vline(rect.left(), rect.y_range(), Stroke::new(2.0, ACCENT));
                } else {
                    index_ui.painter().vline(rect.left(), rect.y_range(), Stroke::new(1.0, RULE));
                }
                let color = if response.hovered() { Color32::WHITE } else if i == current { Color32::WHITE } else { TEXT_DIM };
                index_ui.painter().galley(pos2(rect.left() + 14.0, rect.top() + 4.0), galley, color);
                if response.hovered() {
                    index_ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    guide.scroll_to = Some(heading.clone());
                }
            }
        }
        if let Some(link) = wanted.link {
            guide.follow(&link);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_reads_into_topics() {
        let sections = parse(TEXT_MD);
        assert!(sections.len() >= 10, "{} temas", sections.len());
        for section in &sections {
            assert!(!section.area.is_empty(), "«{}» no tiene área", section.title);
            assert!(!section.lead.is_empty(), "«{}» no empieza con una entradilla", section.title);
            assert!(!section.blocks.is_empty(), "«{}» está vacío", section.title);
        }
    }

    #[test]
    fn its_markdown_becomes_blocks() {
        let text = "Notas del archivo\n\n# Área\n## Uno\nPara qué sirve.\n\nUn párrafo\nque sigue.\n\n### Sub\n- punto\n  sigue\n  - dentro\n1. primero\n2. segundo\n   y más\n\n> Consejo: algo\n> que sigue\n\n> Atención: otro\n\n+++ Cómo funciona\n| A | B |\n|---|---|\n| x | y |\n+++\n#### Menor\n";
        let sections = parse(text);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].area, "Área");
        assert_eq!(sections[0].lead, "Para qué sirve.");
        assert_eq!(
            sections[0].blocks,
            vec![
                Block::Paragraph("Un párrafo que sigue.".into()),
                Block::Heading("Sub".into()),
                Block::Point(0, "punto sigue".into()),
                Block::Point(1, "dentro".into()),
                Block::Step(1, "primero".into()),
                Block::Step(2, "segundo y más".into()),
                Block::Callout(Callout::Tip, "Algo que sigue".into()),
                Block::Callout(Callout::Warning, "Otro".into()),
                Block::Fold("Cómo funciona".into(), vec![Block::Table(vec![vec!["A".into(), "B".into()], vec!["x".into(), "y".into()]])]),
                Block::Minor("Menor".into()),
            ]
        );
    }

    #[test]
    fn lines_have_keys_links_and_bold() {
        let runs = runs("Pulsa {⌘T} y mira [[Acordes|los acordes]] o [[Grabar#Punch in / out]] en **negrita** y `código`");
        let keys: Vec<&str> = runs.iter().filter(|r| r.key).map(|r| r.text.as_str()).collect();
        assert_eq!(keys, vec![theme::keys("⌘T").as_str()]);
        let links: Vec<(&str, &str)> = runs.iter().filter_map(|r| r.link.as_deref().map(|l| (r.text.as_str(), l))).collect();
        assert_eq!(links, vec![("los acordes", "Acordes"), ("Punch in / out", "Grabar#Punch in / out")]);
        assert!(runs.iter().any(|r| r.bold && r.text == "negrita"));
        assert!(runs.iter().any(|r| r.code && r.text == "código"));
        assert_eq!(plain("Pulsa {⌘T} en [[Acordes|los acordes]]"), format!("Pulsa {} en los acordes", theme::keys("⌘T")));
    }

    /// Every [[link]] goes to a topic (and heading) there is, and every fold closes.
    #[test]
    fn every_link_goes_somewhere() {
        let sections = parse(TEXT_MD);
        let mut broken = Vec::new();
        fn headings(blocks: &[Block], out: &mut Vec<String>) {
            for block in blocks {
                match block {
                    Block::Heading(h) | Block::Minor(h) => out.push(plain(h)),
                    Block::Fold(_, inner) => headings(inner, out),
                    _ => {}
                }
            }
        }
        for section in &sections {
            let mut text = section.lead.clone();
            text.push_str(&section.folded_source());
            for run in text.lines().flat_map(runs) {
                let Some(target) = run.link else { continue };
                let (title, heading) = target.split_once('#').map_or((target.as_str(), None), |(t, h)| (t, Some(h)));
                let to = if title.is_empty() { Some(section) } else { sections.iter().find(|s| s.title == title) };
                let ok = to.is_some_and(|to| {
                    let mut hs = Vec::new();
                    headings(&to.blocks, &mut hs);
                    heading.is_none_or(|h| hs.iter().any(|x| x == h))
                });
                if !ok {
                    broken.push(format!("«{}» → [[{target}]]", section.title));
                }
            }
        }
        assert!(broken.is_empty(), "enlaces rotos:\n{}", broken.join("\n"));
        assert_eq!(TEXT_MD.lines().filter(|l| l.trim_start().starts_with("+++")).count() % 2, 0, "un +++ sin cerrar");
    }

    #[test]
    fn the_search_ignores_case_and_accents() {
        let sections = parse(TEXT_MD);
        let titles = |search: &str| matches(&sections, &words(search)).into_iter().map(|i| sections[i].title.clone()).collect::<Vec<_>>();
        assert!(!titles("comping").is_empty());
        assert_eq!(titles("COMPING"), titles("comping"));
        assert_eq!(titles("automatizacion"), titles("automatización"));
        assert!(titles("zarandajo inventado").is_empty());
        // The topic about it comes first.
        assert_eq!(titles("congelar")[0], "Pistas");
        assert_eq!(titles("comping")[0], "Tomas y comping");
        assert_eq!(titles("exportar")[0], "Exportar audio");
    }

    #[test]
    fn every_shortcut_is_in_the_guide() {
        let folded = fold(TEXT_MD);
        for (keys, what) in crate::commands::SHORTCUTS {
            for key in keys.split(" / ") {
                assert!(folded.contains(&fold(key)), "falta el atajo {key} ({what}) en docs/guia.md");
            }
        }
    }

    #[test]
    fn found_words_are_marked() {
        let (job, marks) = line("Graba **tomas** en `loop`", &words("TOMAS"), Style::body());
        let found: Vec<String> = marks.iter().filter(|(_, m)| *m == Mark::Found).map(|(r, _)| job.text.chars().skip(r.start).take(r.len()).collect()).collect();
        assert_eq!(found, vec!["tomas"]);
    }

    /// The page drawn, for clicking through it: a link, the index, back.
    #[test]
    fn links_and_back_move_through_the_guide() {
        let mut guide = Guide::default();
        guide.sections();
        let start = guide.section;
        guide.follow("Acordes");
        let acordes = guide.sections.iter().position(|s| s.title == "Acordes").unwrap();
        assert_eq!(guide.section, acordes);
        assert_eq!(guide.back, vec![start]);
        guide.follow("Grabar#Punch in / out");
        assert_eq!(guide.sections[guide.section].title, "Grabar");
        assert_eq!(guide.scroll_to.as_deref(), Some("Punch in / out"));
    }

    /// Pictures of the page, for looking at its layout:
    /// `PULSE_SHOTS=dir cargo test --release -p pulse -- --ignored guide_shots`.
    #[test]
    #[ignore]
    fn guide_shots() {
        let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        let shots: Vec<(&str, f32, f32, &str, &str, Vec<&str>)> = vec![
            ("primeros", 1440.0, 900.0, "Primeros pasos", "", vec![]),
            ("asistente", 1440.0, 900.0, "Asistente de mezcla y master", "", vec![]),
            ("asistente-abajo", 1440.0, 900.0, "Asistente de mezcla y master", "", vec!["scroll"]),
            ("efectos", 1440.0, 900.0, "Dispositivos y efectos", "", vec!["fold", "scroll"]),
            ("transporte", 1440.0, 900.0, "Transporte", "", vec![]),
            ("atajos", 1440.0, 900.0, "Atajos de teclado", "", vec![]),
            ("buscar", 1440.0, 900.0, "Primeros pasos", "comping", vec![]),
            ("estrecha", 1000.0, 720.0, "Grabar", "", vec![]),
            ("keys", 1440.0, 900.0, "Pulse Keys", "", vec![]),
            ("keys-organo", 1440.0, 900.0, "Pulse Keys", "", vec!["scroll"]),
        ];
        for (name, w, h, topic, search, actions) in shots {
            let ctx = egui::Context::default();
            theme::apply(&ctx);
            let mut app = crate::app::PulseApp::headless(crate::project::Project::empty());
            app.guide.open_at(topic);
            app.guide.search = search.to_string();
            let mut canvas = crate::offscreen::Canvas::default();
            let size = vec2(w, h);
            let input = |events: Vec<egui::Event>| egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() };
            let mut output = None;
            for frame in 0..6 {
                let mut events = Vec::new();
                if frame == 3 && actions.contains(&"scroll") {
                    events.push(egui::Event::PointerMoved(pos2(700.0, 500.0)));
                    events.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, -1400.0), modifiers: Default::default(), phase: egui::TouchPhase::Move });
                }
                if actions.contains(&"fold") {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("guide: every fold open"), true));
                }
                ctx.set_pixels_per_point(2.0);
                output = Some(ctx.run_ui(input(events), |ui| window(&mut app, ui.ctx())));
                if frame < 5 {
                    let out = output.take().unwrap();
                    canvas.draw(&ctx, out, size, 2.0);
                }
            }
            let image = canvas.draw(&ctx, output.unwrap(), size, 2.0);
            image.save_png(&dir.join(format!("{name}.png"))).unwrap();
        }
    }
}
