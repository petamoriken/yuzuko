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
