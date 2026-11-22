use std::{fmt::Write as _, io};

use dialoguer::{
    console::{Key, Term, style},
    theme::Theme,
};

pub struct Item<'a> {
    pub name: &'a str,
    pub languages: Vec<&'a str>,
    pub checked: bool,
    pub language: usize,
}

/// Like `dialoguer::MultiSelect`, but `Tab`/`BackTab` cycle the language of the
/// active item. Returns `(item, language)` pairs of checked items, `None` on quit.
pub fn multi_select(
    theme: &dyn Theme,
    mut items: Vec<Item>,
    max_length: usize,
) -> io::Result<Option<Vec<(usize, usize)>>> {
    let term = Term::stderr();
    if !term.is_term() {
        return Err(io::Error::new(io::ErrorKind::NotConnected, "not a terminal"));
    }
    if items.is_empty() {
        return Err(io::Error::other("Empty list of items"));
    }

    let len = items.len();
    let mut sel = 0usize;
    let mut offset = 0usize;
    let mut drawn = 0usize;
    let mut line = String::new();

    term.hide_cursor()?;
    let res = loop {
        let height = len
            .min(max_length)
            .min((term.size().0 as usize).saturating_sub(1))
            .max(1);
        if sel < offset {
            offset = sel;
        } else if sel >= offset + height {
            offset = sel + 1 - height;
        }

        term.clear_last_lines(drawn)?;
        for (idx, item) in items.iter().enumerate().skip(offset).take(height) {
            line.clear();
            let mut text = String::from(item.name);
            for (i, lang) in item.languages.iter().enumerate() {
                // Languages come after the name, so their reset cannot strip the theme style from it.
                if i == item.language {
                    _ = write!(text, " {}", style(lang).yellow().bold());
                } else {
                    _ = write!(text, " {}", style(lang).yellow());
                }
            }
            _ = theme.format_multi_select_prompt_item(&mut line, &text, item.checked, idx == sel);
            term.write_line(&line)?;
        }
        drawn = height;
        term.flush()?;

        match term.read_key()? {
            Key::ArrowDown | Key::Char('j') => sel = (sel + 1) % len,
            Key::ArrowUp | Key::Char('k') => sel = (sel + len - 1) % len,
            Key::ArrowRight | Key::Char('l') => sel = (sel + height).min(len - 1),
            Key::ArrowLeft | Key::Char('h') => sel = sel.saturating_sub(height),
            Key::Tab => {
                let item = &mut items[sel];
                item.language = (item.language + 1) % item.languages.len();
            }
            Key::BackTab => {
                let item = &mut items[sel];
                let n = item.languages.len();
                item.language = (item.language + n - 1) % n;
            }
            Key::Char(' ') => items[sel].checked = !items[sel].checked,
            Key::Char('a') => {
                let all = items.iter().all(|i| i.checked);
                items.iter_mut().for_each(|i| i.checked = !all);
            }
            Key::Escape | Key::Char('q') => break None,
            Key::Enter => {
                break Some(
                    items
                        .iter()
                        .enumerate()
                        .filter(|(_, i)| i.checked)
                        .map(|(idx, i)| (idx, i.language))
                        .collect(),
                );
            }
            _ => {}
        }
    };
    term.clear_last_lines(drawn)?;
    term.show_cursor()?;
    term.flush()?;
    Ok(res)
}
