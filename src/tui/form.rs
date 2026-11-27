//! Small forms in a popup: text, secret, yes/no and choice fields; long text opens $EDITOR.

use super::{App, Popup};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::rc::Rc;

#[derive(Clone)]
pub enum FieldKind {
    Text,
    Secret,
    /// Multi-line text, edited in $EDITOR (Enter).
    Long,
    Bool,
    Choice(Vec<String>, Vec<String>),
}

#[derive(Clone)]
pub struct Field {
    pub label: String,
    pub value: String,
    pub kind: FieldKind,
    /// Cursor position in chars.
    pub cur: usize,
}

impl Field {
    fn new(label: &str, value: &str, kind: FieldKind) -> Field {
        Field {
            label: label.into(),
            value: value.into(),
            kind,
            cur: value.chars().count(),
        }
    }
    pub fn text(label: &str, value: &str) -> Field {
        Field::new(label, value, FieldKind::Text)
    }
    pub fn secret(label: &str, value: &str) -> Field {
        Field::new(label, value, FieldKind::Secret)
    }
    pub fn long(label: &str, value: &str) -> Field {
        Field::new(label, value, FieldKind::Long)
    }
    pub fn bool(label: &str, v: bool) -> Field {
        Field::new(label, if v { "true" } else { "false" }, FieldKind::Bool)
    }
    pub fn choice(label: &str, options: &[&str], cur: &str) -> Field {
        let opts: Vec<String> = options.iter().map(|s| s.to_string()).collect();
        let v = if opts.iter().any(|o| o == cur) {
            cur.to_string()
        } else {
            opts[0].clone()
        };
        Field::new(label, &v, FieldKind::Choice(opts, vec![]))
    }
    /// Descriptions shown for the options of a choice.
    pub fn describe(mut self, d: &[String]) -> Field {
        if let FieldKind::Choice(_, desc) = &mut self.kind {
            *desc = d.to_vec();
        }
        self
    }
    /// What the field shows.
    pub fn shown(&self) -> String {
        match &self.kind {
            FieldKind::Secret => "•".repeat(self.value.chars().count()),
            FieldKind::Bool => if self.value == "true" {
                "[x] yes"
            } else {
                "[ ] no"
            }
            .into(),
            FieldKind::Choice(opts, desc) => {
                let i = opts.iter().position(|o| *o == self.value).unwrap_or(0);
                let d = desc.get(i).map(|d| format!(" — {d}")).unwrap_or_default();
                format!("‹ {}{} ›", self.value, d)
            }
            FieldKind::Long => {
                let first = self.value.lines().next().unwrap_or("");
                let n = self.value.lines().count();
                if self.value.is_empty() {
                    "(Enter: edit)".into()
                } else if n > 1 {
                    format!("{first} … ({n} lines, Enter: edit)")
                } else {
                    format!("{first}  (Enter: edit)")
                }
            }
            FieldKind::Text => self.value.clone(),
        }
    }
}

type Submit = Rc<dyn Fn(&mut App, &[String]) -> Result<(), String>>;
type Cancel = Rc<dyn Fn(&mut App)>;

pub struct Form {
    pub title: String,
    pub submit: String,
    pub notes: Vec<String>,
    pub fields: Vec<Field>,
    pub sel: usize,
    pub error: Option<String>,
    on_submit: Option<Submit>,
    on_cancel: Option<Cancel>,
}

impl Form {
    pub fn new(title: impl Into<String>, submit: &str) -> Form {
        Form {
            title: title.into(),
            submit: submit.into(),
            notes: vec![],
            fields: vec![],
            sel: 0,
            error: None,
            on_submit: None,
            on_cancel: None,
        }
    }
    pub fn note(mut self, s: &str) -> Form {
        self.notes.push(s.into());
        self
    }
    pub fn field(mut self, f: Field) -> Form {
        self.fields.push(f);
        self
    }
    pub fn on_submit(
        mut self,
        f: impl Fn(&mut App, &[String]) -> Result<(), String> + 'static,
    ) -> Form {
        self.on_submit = Some(Rc::new(f));
        self
    }
    /// Called when the form is closed with Esc.
    pub fn on_cancel(mut self, f: impl Fn(&mut App) + 'static) -> Form {
        self.on_cancel = Some(Rc::new(f));
        self
    }
}

/// The form and field a long field is being edited for.
pub struct LongEdit {
    pub form: Form,
    pub field: usize,
    pub file: std::path::PathBuf,
}

fn submit(app: &mut App, mut f: Form) {
    let vals: Vec<String> = f.fields.iter().map(|x| x.value.clone()).collect();
    let Some(cb) = f.on_submit.clone() else {
        return;
    };
    if let Err(e) = cb(app, &vals) {
        f.error = Some(e);
        app.popup = Some(Popup::Form(f));
    }
}

pub fn key(app: &mut App, mut f: Form, k: KeyEvent) {
    let n = f.fields.len();
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    if k.code == KeyCode::Esc {
        if let Some(cb) = f.on_cancel.clone() {
            cb(app);
        }
        return;
    }
    let Some(field) = f.fields.get_mut(f.sel) else {
        return submit(app, f);
    };
    f.error = None;
    match (k.code, &field.kind) {
        (KeyCode::Char('s'), _) if ctrl => return submit(app, f),
        (KeyCode::Tab | KeyCode::Down, _) => f.sel = (f.sel + 1) % n,
        (KeyCode::BackTab | KeyCode::Up, _) => f.sel = (f.sel + n - 1) % n,
        (KeyCode::Enter, FieldKind::Long) => {
            let file = super::compose::scratch_file("field.md");
            if let Err(e) = std::fs::write(&file, &field.value) {
                app.error(e);
                return;
            }
            let idx = f.sel;
            app.long_edit = Some(LongEdit {
                form: f,
                field: idx,
                file,
            });
            return;
        }
        (KeyCode::Enter, _) => {
            if f.sel + 1 < n {
                f.sel += 1;
            } else {
                return submit(app, f);
            }
        }
        (KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right, FieldKind::Bool) => {
            field.value = if field.value == "true" {
                "false".into()
            } else {
                "true".into()
            };
        }
        (KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right, FieldKind::Choice(opts, _)) => {
            let i = opts.iter().position(|o| *o == field.value).unwrap_or(0);
            let j = if k.code == KeyCode::Left {
                (i + opts.len() - 1) % opts.len()
            } else {
                (i + 1) % opts.len()
            };
            field.value = opts[j].clone();
        }
        (_, FieldKind::Bool | FieldKind::Choice(..) | FieldKind::Long) => {}
        (KeyCode::Char('u'), _) if ctrl => {
            field.value.clear();
            field.cur = 0;
        }
        (KeyCode::Char('a'), _) if ctrl => field.cur = 0,
        (KeyCode::Char('e'), _) if ctrl => field.cur = field.value.chars().count(),
        (KeyCode::Char(c), _) => {
            let i = byte_at(&field.value, field.cur);
            field.value.insert(i, c);
            field.cur += 1;
        }
        (KeyCode::Backspace, _) if field.cur > 0 => {
            let i = byte_at(&field.value, field.cur - 1);
            field.value.remove(i);
            field.cur -= 1;
        }
        (KeyCode::Delete, _) if field.cur < field.value.chars().count() => {
            let i = byte_at(&field.value, field.cur);
            field.value.remove(i);
        }
        (KeyCode::Left, _) => field.cur = field.cur.saturating_sub(1),
        (KeyCode::Right, _) => field.cur = (field.cur + 1).min(field.value.chars().count()),
        (KeyCode::Home, _) => field.cur = 0,
        (KeyCode::End, _) => field.cur = field.value.chars().count(),
        _ => {}
    }
    app.popup = Some(Popup::Form(f));
}

fn byte_at(s: &str, chars: usize) -> usize {
    s.char_indices()
        .nth(chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}
