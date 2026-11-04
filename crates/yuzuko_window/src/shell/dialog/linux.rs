//! GTK dialogs for one window.

pub(super) fn present(
  title: &str,
  message: &str,
  dialog: super::Dialog<'_>,
) -> Result<super::Answer, String> {
  use gtk::prelude::*;
  crate::shell::gtk_loop::ready()?;
  match dialog {
    super::Dialog::Alert => {
      let alert = gtk::MessageDialog::builder()
        .modal(true)
        .buttons(gtk::ButtonsType::Ok)
        .text(message)
        .title(title)
        .build();
      alert.run();
      alert.hide();
      Ok(super::Answer::Ok)
    }
    super::Dialog::Confirm => {
      let alert = gtk::MessageDialog::builder()
        .modal(true)
        .buttons(gtk::ButtonsType::OkCancel)
        .text(message)
        .title(title)
        .build();
      let response = alert.run();
      alert.hide();
      if response == gtk::ResponseType::Ok {
        Ok(super::Answer::Ok)
      } else {
        Ok(super::Answer::Cancel)
      }
    }
    super::Dialog::Prompt(default_value) => {
      let alert = gtk::Dialog::with_buttons(
        Some(title),
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        &[
          ("Cancel", gtk::ResponseType::Cancel),
          ("OK", gtk::ResponseType::Ok),
        ],
      );
      let label = gtk::Label::new(Some(message));
      let entry = gtk::Entry::new();
      entry.set_text(default_value);
      let content = alert.content_area();
      content.add(&label);
      content.add(&entry);
      alert.set_default_response(gtk::ResponseType::Ok);
      alert.show_all();
      let response = alert.run();
      let text = entry.text().to_string();
      alert.hide();
      if response == gtk::ResponseType::Ok {
        Ok(super::Answer::Text(text))
      } else {
        Ok(super::Answer::Cancel)
      }
    }
  }
}

pub(super) fn pick_files(
  title: &str,
  multiple: bool,
  filters: &[super::FileFilter],
) -> Result<Option<Vec<String>>, String> {
  choose(title, false, multiple, None, filters)
}

pub(super) fn pick_save(
  title: &str,
  suggested: Option<&str>,
  filters: &[super::FileFilter],
) -> Result<Option<Vec<String>>, String> {
  choose(title, true, false, suggested, filters)
}

/// GTK 3 file chooser. It is not parented to the winit window, same as alert.
fn choose(
  title: &str,
  save: bool,
  multiple: bool,
  suggested: Option<&str>,
  filters: &[super::FileFilter],
) -> Result<Option<Vec<String>>, String> {
  use gtk::prelude::*;
  crate::shell::gtk_loop::ready()?;
  let action = if save {
    gtk::FileChooserAction::Save
  } else {
    gtk::FileChooserAction::Open
  };
  let dialog = gtk::FileChooserDialog::builder()
    .title(title)
    .action(action)
    .modal(true)
    .build();
  dialog.add_button("Cancel", gtk::ResponseType::Cancel);
  dialog.add_button(
    if save { "Save" } else { "Open" },
    gtk::ResponseType::Accept,
  );
  dialog.set_default_response(gtk::ResponseType::Accept);
  if multiple {
    dialog.set_select_multiple(true);
  }
  if save {
    dialog.set_do_overwrite_confirmation(true);
    if let Some(name) = suggested {
      dialog.set_current_name(name);
    }
  }
  add_filters(&dialog, filters);
  dialog.show_all();
  let response = dialog.run();
  let paths = if response == gtk::ResponseType::Accept {
    let picked = if multiple {
      dialog.filenames()
    } else {
      dialog.filename().into_iter().collect::<Vec<_>>()
    };
    let paths: Vec<String> =
      picked.into_iter().map(super::absolute_path).collect();
    if paths.is_empty() { None } else { Some(paths) }
  } else {
    None
  };
  dialog.hide();
  Ok(paths)
}

fn add_filters(dialog: &gtk::FileChooserDialog, filters: &[super::FileFilter]) {
  use gtk::prelude::*;
  if filters.is_empty() {
    return;
  }
  for filter in filters {
    let native = gtk::FileFilter::new();
    native.set_name(Some(&filter.name));
    if filter.all {
      native.add_pattern("*");
    } else {
      for extension in &filter.extensions {
        native.add_pattern(&format!("*.{extension}"));
      }
    }
    dialog.add_filter(native);
  }
}
