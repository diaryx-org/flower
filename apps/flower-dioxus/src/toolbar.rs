//! The buttons at the end of the editor's bar: how much of the document
//! inlines onto a page, and Undo, Redo and Delete — the Swift app's toolbar,
//! in unit's controls.
//!
//! How much inlines is about the room the pages are drawn in, not about any
//! one file, so it is the window's choice and not the document's: Fit sizes
//! it to the window and follows a resize (the editor's default); Pages is the
//! settings-menu rule; Roomy inlines a couple of ranks; Flat puts the whole
//! document on one page.

use dioxus::prelude::*;
use flower_core::{InlineBudget, Mode};
use flower_dioxus::Editor;
use unit::icon::paths;
use unit::{Button, Segmented, Variant, use_unit};

/// The budgets on offer, after Fit: `(name, rows, depth)`.
const BUDGETS: [(&str, usize, usize); 3] = [("Pages", 6, 1), ("Roomy", 24, 2), ("Flat", 9999, 99)];

/// Which of the toolbar's budgets `budget` is: 0 for Fit.
pub fn budget_index(budget: Option<InlineBudget>) -> usize {
    budget
        .and_then(|b| {
            BUDGETS
                .iter()
                .position(|&(_, rows, depth)| InlineBudget::new(rows, depth) == b)
        })
        .map_or(0, |i| i + 1)
}

/// The budget the toolbar's `n`th option stands for.
pub fn budget_of(n: usize) -> Option<InlineBudget> {
    let (_, rows, depth) = *BUDGETS.get(n.checked_sub(1)?)?;
    Some(InlineBudget::new(rows, depth))
}

#[component]
pub fn Toolbar(editor: Editor) -> Element {
    let unit = use_unit();
    // Read so the buttons follow the model.
    let _ = editor.changed();
    let (normal, item, undo, redo) = {
        let m = editor.model();
        let normal = matches!(m.mode, Mode::Normal);
        (
            normal,
            normal && m.page_item().is_some(),
            normal && m.history_len() > 0,
            normal && m.redo_len() > 0,
        )
    };
    let selected = budget_index(editor.budget());
    let mut options = vec!["Fit".to_string()];
    options.extend(BUDGETS.iter().map(|(n, ..)| n.to_string()));
    let (b, u, r, d) = (
        editor.clone(),
        editor.clone(),
        editor.clone(),
        editor.clone(),
    );
    rsx! {
        Segmented { options, selected: Some(selected), label: "Inline",
            disabled: !normal,
            onchange: move |n: usize| {
                b.set_budget(budget_of(n));
                unit.blur();
            }
        }
        Button { variant: Variant::Ghost, class: "unit-icon", icon: paths::UNDO, label: "Undo",
                 disabled: !undo, refuses_keys: true, onpress: move |_| u.undo() }
        Button { variant: Variant::Ghost, class: "unit-icon flower-redo", icon: paths::UNDO,
                 label: "Redo", disabled: !redo, refuses_keys: true, onpress: move |_| r.redo() }
        Button { variant: Variant::Ghost, class: "unit-icon", icon: paths::TRASH, label: "Delete",
                 disabled: !item, refuses_keys: true,
                 onpress: move |_| {
                     let path = d.model().page_item().map(|i| i.path.clone());
                     if let Some(p) = path {
                         d.delete(p);
                     }
                 } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_budget_is_its_own_option_and_fit_is_none() {
        assert_eq!(budget_index(None), 0);
        assert_eq!(budget_of(0), None);
        for n in 1..=BUDGETS.len() {
            assert_eq!(budget_index(budget_of(n)), n);
        }
    }
}
