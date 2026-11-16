// SPDX-License-Identifier: MIT OR Apache-2.0
//! Source drafts are view state; only Apply/Ctrl+Enter emits a source edit.
#[derive(Clone, Debug)]
pub struct ExpressionDraft {
    pub accepted_text: String,
    pub draft_text: String,
    pub diagnostic: Option<String>,
    pub base_revision: u64,
    pub conflict: bool,
    focused: bool,
}
impl ExpressionDraft {
    pub fn new(text: &str, revision: u64) -> Self {
        Self {
            accepted_text: text.into(),
            draft_text: text.into(),
            diagnostic: None,
            base_revision: revision,
            conflict: false,
            focused: false,
        }
    }
    pub fn reconcile(&mut self, text: &str, revision: u64) {
        if text == self.accepted_text && self.draft_text != text {
            self.base_revision = revision;
            return;
        }
        if self.draft_text == text {
            self.accepted_text = text.into();
            self.conflict = false;
            self.diagnostic = None;
        } else if self.focused || self.draft_text != self.accepted_text {
            self.accepted_text = text.into();
            self.conflict = true;
            self.diagnostic = Some(
                "Source changed while this draft was open. Reload or keep draft explicitly.".into(),
            );
        } else {
            self.accepted_text = text.into();
            self.draft_text = text.into();
            self.diagnostic = None;
        }
        self.base_revision = revision;
    }
    pub fn commit(&mut self) -> Option<String> {
        if self.conflict {
            return None;
        }
        let text = if self.draft_text.trim_start().starts_with('=') {
            self.draft_text.clone()
        } else {
            format!("= {}", self.draft_text)
        };
        match wisp::core::MathFunctionParser::parse_function_checked(&text) {
            Ok(_) => {
                self.diagnostic = None;
                (text != self.accepted_text).then_some(text)
            }
            Err(error) => {
                self.diagnostic = Some(error.to_string());
                None
            }
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, width: f32| {
            let mut job = highlight(text.as_str());
            job.wrap.max_width = width;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        let response = ui.add(
            egui::TextEdit::multiline(&mut self.draft_text)
                .id_salt("expression_draft")
                .desired_width(240.)
                .desired_rows(3)
                .char_limit(8192)
                .layouter(&mut layouter),
        );
        self.focused = response.has_focus();
        if self.focused && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.draft_text = self.accepted_text.clone();
            self.diagnostic = None;
            self.conflict = false;
            response.surrender_focus();
        }
        let mut apply = self.focused
            && ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            apply |= ui
                .add_enabled(!self.conflict, egui::Button::new("Apply"))
                .clicked();
            ui.small("Ctrl/Cmd+Enter applies · Enter adds a line");
        });
        if self.conflict {
            ui.horizontal(|ui| {
                if ui.button("Reload source").clicked() {
                    self.draft_text = self.accepted_text.clone();
                    self.conflict = false;
                    self.diagnostic = None;
                }
                if ui.button("Keep draft").clicked() {
                    self.conflict = false;
                    self.diagnostic = None;
                }
            });
        }
        if let Some(diagnostic) = &self.diagnostic {
            ui.colored_label(egui::Color32::LIGHT_RED, diagnostic);
        }
        if apply { self.commit() } else { None }
    }
}
/// Highlight source without changing bytes or pretending to validate grammar.
pub fn highlight(text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut start = 0;
    let kind = |ch: char| {
        if ch.is_ascii_digit() || ch == '.' {
            0
        } else if ch == '$' {
            1
        } else if ch.is_alphabetic() || ch == '_' {
            2
        } else if "+-*/%<>=!&|^()".contains(ch) {
            3
        } else {
            4
        }
    };
    while start < text.len() {
        let first = text[start..].chars().next().unwrap();
        let group = kind(first);
        let mut end = start + first.len_utf8();
        for ch in text[end..].chars() {
            if kind(ch) != group {
                break;
            }
            end += ch.len_utf8();
        }
        let color = match group {
            0 => egui::Color32::LIGHT_BLUE,
            1 => egui::Color32::LIGHT_GREEN,
            2 => egui::Color32::LIGHT_YELLOW,
            3 => egui::Color32::LIGHT_RED,
            _ => egui::Color32::LIGHT_GRAY,
        };
        job.append(
            &text[start..end],
            0.,
            egui::TextFormat {
                font_id: egui::FontId::monospace(13.),
                color,
                ..Default::default()
            },
        );
        start = end;
    }
    job
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_drafts_and_external_conflicts_are_retained() {
        let mut draft = ExpressionDraft::new("= $0 + 1", 1);
        draft.draft_text = "= (".into();
        assert!(draft.commit().is_none());
        assert!(draft.diagnostic.is_some());
        draft.reconcile("= $0 + 2", 2);
        assert!(draft.conflict);
        assert_eq!(draft.draft_text, "= (");
        assert!(draft.commit().is_none());
        draft.draft_text = "= $0 + 2".into();
        draft.reconcile("= $0 + 2", 3);
        assert!(!draft.conflict);
        assert_eq!(draft.accepted_text, "= $0 + 2");
        let text = "= $0 * 1e-3\n + SampleRate / 2 // λ";
        assert_eq!(highlight(text).text, text);
    }
}
