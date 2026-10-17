//! Ayuda → Guía de Pulse: everything the app does, by topic, with a search box.
//!
//! The text is `docs/guia.md`, built into the app. It is a small subset of
//! Markdown: `## ` starts a section (a topic in the list), `### ` a heading inside
//! it, `- ` a point (indented two spaces: a point inside the one before), `| a | b |`
//! a row of a table, and **bold** and `code` inside lines. Blank lines separate
//! paragraphs.

use crate::app::PulseApp;
use crate::theme::{self, ACCENT, TEXT, TEXT_DIM};
use eframe::egui::{self, text::LayoutJob, Color32, FontId, TextFormat};

const TEXT_MD: &str = include_str!("../../../docs/guia.md");

#[derive(Clone, Debug, PartialEq)]
enum Block {
    Heading(String),
    Paragraph(String),
    /// A point and how deep it is (0: top level).
    Point(usize, String),
    Table(Vec<Vec<String>>),
}

#[derive(Clone, Debug)]
struct Section {
    title: String,
    blocks: Vec<Block>,
    /// All of it folded (see `fold`), for the search.
    folded: String,
}

#[derive(Default)]
pub struct Guide {
    pub open: bool,
    search: String,
    /// The search the topic was chosen for (a new search picks its best topic).
    searched: String,
    section: usize,
    sections: Vec<Section>,
}

fn parse(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut paragraph = String::new();
    fn flush(sections: &mut [Section], paragraph: &mut String) {
        if let (Some(section), false) = (sections.last_mut(), paragraph.trim().is_empty()) {
            section.blocks.push(Block::Paragraph(paragraph.trim().to_string()));
        }
        paragraph.clear();
    }
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(title) = line.strip_prefix("## ") {
            flush(&mut sections, &mut paragraph);
            sections.push(Section { title: title.trim().to_string(), blocks: Vec::new(), folded: String::new() });
            continue;
        }
        // The document's title, and anything before the first section.
        if line.starts_with("# ") || sections.is_empty() {
            continue;
        }
        let section = sections.len() - 1;
        if let Some(heading) = line.strip_prefix("### ") {
            flush(&mut sections, &mut paragraph);
            sections[section].blocks.push(Block::Heading(heading.trim().to_string()));
        } else if let Some(point) = trimmed.strip_prefix("- ") {
            flush(&mut sections, &mut paragraph);
            let depth = (line.len() - trimmed.len()) / 2;
            sections[section].blocks.push(Block::Point(depth, point.trim().to_string()));
        } else if trimmed.starts_with('|') {
            flush(&mut sections, &mut paragraph);
            let cells: Vec<String> = trimmed.trim_matches('|').split('|').map(|c| c.trim().to_string()).collect();
            // The |---|---| line under the header.
            if cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':')) {
                continue;
            }
            match sections[section].blocks.last_mut() {
                Some(Block::Table(rows)) => rows.push(cells),
                _ => sections[section].blocks.push(Block::Table(vec![cells])),
            }
        } else if trimmed.is_empty() {
            flush(&mut sections, &mut paragraph);
        } else if let Some(Block::Point(_, point)) = sections[section].blocks.last_mut().filter(|_| paragraph.is_empty() && line.starts_with("  ")) {
            // A point that goes on on the next line.
            point.push(' ');
            point.push_str(trimmed);
        } else {
            paragraph.push_str(trimmed);
            paragraph.push(' ');
        }
    }
    flush(&mut sections, &mut paragraph);
    for section in &mut sections {
        let mut all = section.title.clone();
        for block in &section.blocks {
            all.push('\n');
            match block {
                Block::Heading(t) | Block::Paragraph(t) | Block::Point(_, t) => all.push_str(t),
                Block::Table(rows) => all.push_str(&rows.iter().map(|r| r.join(" ")).collect::<Vec<_>>().join("\n")),
            }
        }
        section.folded = fold(&all.replace("**", "").replace('`', ""));
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

/// A line with its **bold** and `code`, the words searched for highlighted.
fn line(text: &str, words: &[String], size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    // Split into runs: (text, bold, code).
    let mut runs: Vec<(String, bool, bool)> = Vec::new();
    let (mut bold, mut code) = (false, false);
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '*' && chars.peek() == Some(&'*') && !code {
            chars.next();
            runs.push((std::mem::take(&mut current), bold, code));
            bold = !bold;
        } else if c == '`' {
            runs.push((std::mem::take(&mut current), bold, code));
            code = !code;
        } else {
            current.push(c);
        }
    }
    runs.push((current, bold, code));
    for (run, bold, code) in runs.into_iter().filter(|r| !r.0.is_empty()) {
        let format = TextFormat {
            font_id: if code { FontId::monospace(size - 1.0) } else { FontId::proportional(size) },
            color: if bold { Color32::WHITE } else if code { ACCENT } else { color },
            ..Default::default()
        };
        // Where the words searched for are, in characters.
        let folded: Vec<char> = run.chars().map(fold_char).collect();
        let mut marked = vec![false; folded.len()];
        for word in words {
            let word: Vec<char> = word.chars().collect();
            if word.is_empty() || word.len() > folded.len() {
                continue;
            }
            for start in 0..=folded.len() - word.len() {
                if folded[start..start + word.len()] == word[..] {
                    marked[start..start + word.len()].iter_mut().for_each(|m| *m = true);
                }
            }
        }
        let chars: Vec<char> = run.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let mark = marked[i];
            let mut j = i;
            while j < chars.len() && marked[j] == mark {
                j += 1;
            }
            let piece: String = chars[i..j].iter().collect();
            let mut format = format.clone();
            if mark {
                format.background = ACCENT.gamma_multiply(0.45);
                format.color = Color32::WHITE;
            }
            job.append(&piece, 0.0, format);
            i = j;
        }
    }
    job
}

fn show_section(ui: &mut egui::Ui, section: &Section, words: &[String]) {
    ui.label(line(&section.title, words, 18.0, Color32::WHITE));
    ui.add_space(6.0);
    for block in &section.blocks {
        match block {
            Block::Heading(text) => {
                ui.add_space(8.0);
                ui.label(line(text, words, 14.5, ACCENT));
                ui.add_space(2.0);
            }
            Block::Paragraph(text) => {
                ui.label(line(text, words, 13.0, TEXT));
                ui.add_space(4.0);
            }
            Block::Point(depth, text) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(4.0 + *depth as f32 * 16.0);
                    ui.label(egui::RichText::new(if *depth == 0 { "•" } else { "◦" }).color(TEXT_DIM));
                    ui.add(egui::Label::new(line(text, words, 13.0, TEXT)).wrap());
                });
                ui.add_space(2.0);
            }
            Block::Table(rows) => {
                ui.add_space(2.0);
                egui::Grid::new(ui.next_auto_id()).striped(true).num_columns(rows.first().map_or(2, Vec::len)).spacing([16.0, 4.0]).show(ui, |ui| {
                    for (r, row) in rows.iter().enumerate() {
                        for cell in row {
                            let color = if r == 0 { TEXT_DIM } else { TEXT };
                            ui.add(egui::Label::new(line(cell, words, 13.0, color)).wrap());
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
            }
        }
    }
}

pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    let guide = &mut app.guide;
    if !guide.open {
        return;
    }
    if guide.sections.is_empty() {
        guide.sections = parse(TEXT_MD);
    }
    let mut open = guide.open;
    egui::Window::new("Guía de Pulse")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([880.0, 620.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            let search = egui::TextEdit::singleline(&mut guide.search).hint_text("Buscar en la guía… (por ejemplo: tomas, congelar, MIDI)").desired_width(f32::INFINITY);
            let response = ui.add(search);
            crate::widgets::name(&response, egui::WidgetType::TextEdit, "Buscar en la guía");
            ui.add_space(6.0);
            let words = words(&guide.search);
            let found = matches(&guide.sections, &words);
            if guide.searched != guide.search || !found.contains(&guide.section) {
                guide.searched.clone_from(&guide.search);
                if let Some(&first) = found.first().filter(|_| !words.is_empty() || !found.contains(&guide.section)) {
                    guide.section = first;
                }
            }
            let height = 540.0;
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(210.0);
                    egui::ScrollArea::vertical().id_salt("guide topics").max_height(height).show(ui, |ui| {
                        if found.is_empty() {
                            ui.label(egui::RichText::new("Nada encontrado").color(TEXT_DIM));
                        }
                        for &i in &found {
                            let title = &guide.sections[i].title;
                            if ui.selectable_label(guide.section == i, title).clicked() {
                                guide.section = i;
                            }
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical().id_salt(("guide text", guide.section)).max_height(height).auto_shrink(false).show(ui, |ui| {
                        if let Some(section) = guide.sections.get(guide.section).filter(|_| !found.is_empty()) {
                            show_section(ui, section, &words);
                        }
                    });
                });
            });
            ui.label(egui::RichText::new(format!("{} temas", guide.sections.len())).color(TEXT_DIM).font(theme::small()));
        });
    app.guide.open = open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_reads_into_topics() {
        let sections = parse(TEXT_MD);
        assert!(sections.len() >= 10, "{} temas", sections.len());
        for section in &sections {
            assert!(!section.blocks.is_empty(), "«{}» está vacío", section.title);
        }
    }

    #[test]
    fn its_markdown_becomes_blocks() {
        let sections = parse("# Título\n\nnada\n\n## Uno\nUn párrafo\nque sigue.\n\n### Sub\n- punto\n  sigue\n  - dentro\n\n| A | B |\n|---|---|\n| x | y |\n");
        assert_eq!(sections.len(), 1);
        assert_eq!(
            sections[0].blocks,
            vec![
                Block::Paragraph("Un párrafo que sigue.".into()),
                Block::Heading("Sub".into()),
                Block::Point(0, "punto sigue".into()),
                Block::Point(1, "dentro".into()),
                Block::Table(vec![vec!["A".into(), "B".into()], vec!["x".into(), "y".into()]]),
            ]
        );
    }

    #[test]
    fn the_search_ignores_case_and_accents() {
        let sections = parse(TEXT_MD);
        let titles = |search: &str| matches(&sections, &words(search)).into_iter().map(|i| sections[i].title.clone()).collect::<Vec<_>>();
        assert!(!titles("comping").is_empty());
        assert_eq!(titles("COMPING"), titles("comping"));
        assert_eq!(titles("automatizacion"), titles("automatización"));
        assert!(titles("palabra que no está en ninguna parte").is_empty());
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
    fn found_words_are_highlighted() {
        let job = line("Graba **tomas** en `loop`", &words("TOMAS"), 13.0, TEXT);
        let marked: Vec<&str> = job.sections.iter().filter(|s| s.format.background != Color32::TRANSPARENT).map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0]).collect();
        assert_eq!(marked, vec!["tomas"]);
    }
}
