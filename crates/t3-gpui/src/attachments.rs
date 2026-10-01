//! Reusable composer attachment tray.
//!
//! File dialogs and byte transfers belong to the app/backend. This entity
//! keeps selected paths and upload state, emits work requests, and accepts
//! completion only when its request id still matches the row.

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::attachments::{LocalAttachment, UploadedAttachment, validate_selection};

#[derive(Debug, Clone)]
pub enum AttachmentPanelEvent {
    ChooseFiles,
    UploadRequested { request_id: u64, attachment: LocalAttachment },
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentStatus {
    Uploading,
    Uploaded(UploadedAttachment),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct AttachmentRow {
    pub attachment: LocalAttachment,
    pub request_id: u64,
    pub status: AttachmentStatus,
}

pub struct AttachmentPanel {
    rows: Vec<AttachmentRow>,
    next_request_id: u64,
    connected: bool,
}

impl EventEmitter<AttachmentPanelEvent> for AttachmentPanel {}

impl AttachmentPanel {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self { rows: Vec::new(), next_request_id: 0, connected: false }
    }

    #[cfg(test)]
    pub fn rows(&self) -> &[AttachmentRow] {
        &self.rows
    }

    /// Metadata ready to attach to `thread.turn.start`; only fully uploaded
    /// items are returned, in composer order.
    pub fn uploaded_attachments(&self) -> Vec<UploadedAttachment> {
        self.rows
            .iter()
            .filter_map(|row| match &row.status {
                AttachmentStatus::Uploaded(attachment) => Some(attachment.clone()),
                _ => None,
            })
            .collect()
    }

    /// Sending is enabled when the tray is empty or every selected file is
    /// uploaded. A failed row remains selected until retry or removal.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn can_send(&self) -> bool {
        self.connected
            && self.rows.iter().all(|row| matches!(&row.status, AttachmentStatus::Uploaded(_)))
    }

    /// Clear only the exact server attachment ids included in an accepted
    /// send. Files added while that send was pending remain in the tray.
    pub fn clear_uploaded(&mut self, sent_ids: &[String], cx: &mut Context<Self>) {
        if sent_ids.is_empty() {
            return;
        }
        let before = self.rows.len();
        self.rows.retain(|row| match &row.status {
            AttachmentStatus::Uploaded(uploaded) => !sent_ids.iter().any(|id| id == &uploaded.id),
            AttachmentStatus::Uploading | AttachmentStatus::Failed(_) => true,
        });
        if before != self.rows.len() {
            cx.notify();
        }
    }

    pub fn clear_sent(&mut self, sent_ids: &[String], cx: &mut Context<Self>) {
        self.clear_uploaded(sent_ids, cx);
    }

    pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        if self.connected == connected {
            return;
        }
        self.connected = connected;
        if !connected {
            self.fail_pending("Connection lost. Reconnect and retry the upload.", cx);
        } else {
            cx.notify();
        }
    }

    pub fn fail_pending(&mut self, reason: &str, cx: &mut Context<Self>) {
        let mut changed = false;
        for row in &mut self.rows {
            if matches!(&row.status, AttachmentStatus::Uploading) {
                self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
                row.request_id = self.next_request_id;
                row.status = AttachmentStatus::Failed(reason.to_owned());
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// Called by the app after its native file chooser returns selected paths.
    /// Valid paths are retained and uploaded immediately; rejected paths leave
    /// the existing selection intact.
    pub fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>, cx: &mut Context<Self>) {
        if !self.connected {
            cx.emit(AttachmentPanelEvent::Rejected(
                "Connect to a server before attaching files.".into(),
            ));
            return;
        }
        let mut accepted = Vec::new();
        for path in paths {
            match LocalAttachment::from_path(path) {
                Ok(attachment) => accepted.push(attachment),
                Err(error) => cx.emit(AttachmentPanelEvent::Rejected(error.to_string())),
            }
        }
        if accepted.is_empty() {
            return;
        }
        let mut selection: Vec<_> = self.rows.iter().map(|row| row.attachment.clone()).collect();
        selection.extend(accepted.iter().cloned());
        if let Err(error) = validate_selection(&selection) {
            cx.emit(AttachmentPanelEvent::Rejected(error.to_string()));
            return;
        }

        for attachment in accepted {
            let request_id = self.allocate_request_id();
            self.rows.push(AttachmentRow {
                attachment: attachment.clone(),
                request_id,
                status: AttachmentStatus::Uploading,
            });
            cx.emit(AttachmentPanelEvent::UploadRequested { request_id, attachment });
        }
        cx.notify();
    }

    pub fn remove(&mut self, local_id: &str, cx: &mut Context<Self>) {
        let before = self.rows.len();
        self.rows.retain(|row| row.attachment.id != local_id);
        if self.rows.len() != before {
            cx.notify();
        }
    }

    pub fn retry(&mut self, local_id: &str, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        let Some(index) = self.rows.iter().position(|row| row.attachment.id == local_id) else {
            return;
        };
        let request_id = self.allocate_request_id();
        let row = &mut self.rows[index];
        row.request_id = request_id;
        row.status = AttachmentStatus::Uploading;
        let attachment = row.attachment.clone();
        cx.emit(AttachmentPanelEvent::UploadRequested { request_id, attachment });
        cx.notify();
    }

    /// Ignore a completion from an upload that was removed or retried since
    /// it began. This also protects a panel reused after a thread switch.
    pub fn complete_upload(
        &mut self,
        local_id: &str,
        request_id: u64,
        result: Result<UploadedAttachment, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.attachment.id == local_id && row.request_id == request_id)
        else {
            return;
        };
        row.status = match result {
            Ok(uploaded) => AttachmentStatus::Uploaded(uploaded),
            Err(error) => AttachmentStatus::Failed(error),
        };
        cx.notify();
    }

    fn allocate_request_id(&mut self) -> u64 {
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.next_request_id
    }
}

impl Render for AttachmentPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<_> = self
            .rows
            .iter()
            .map(|row| render_row(row, self.connected, cx).into_any_element())
            .collect();
        v_flex().gap_1().children(rows)
    }
}

fn render_row(
    row: &AttachmentRow,
    connected: bool,
    cx: &mut Context<AttachmentPanel>,
) -> impl IntoElement {
    let theme = cx.theme();
    let local_id = row.attachment.id.clone();
    let remove_id = local_id.clone();
    let retry_id = local_id;
    let (status, status_color, retryable) = match &row.status {
        AttachmentStatus::Uploading => ("Uploading…".to_owned(), theme.muted_foreground, false),
        AttachmentStatus::Uploaded(_) => ("Ready".to_owned(), theme.primary, false),
        AttachmentStatus::Failed(error) => (format!("Upload failed · {error}"), theme.danger, true),
    };

    h_flex()
        .id(SharedString::from(format!("composer-attachment-{remove_id}")))
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_sm().truncate().child(row.attachment.name.clone()))
                .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                    "{} · {}",
                    format_size(row.attachment.size_bytes),
                    status
                )))
                .text_color(theme.foreground),
        )
        .when(retryable, |row_element| {
            row_element.child(
                Button::new(SharedString::from(format!("composer-attachment-retry-{retry_id}")))
                    .ghost()
                    .small()
                    .label("Retry")
                    .disabled(!connected)
                    .on_click(cx.listener(move |this, _, _, cx| this.retry(&retry_id, cx))),
            )
        })
        .child(div().text_xs().text_color(status_color).child(match &row.status {
            AttachmentStatus::Uploaded(_) => "✓",
            AttachmentStatus::Failed(_) => "!",
            AttachmentStatus::Uploading => "·",
        }))
        .child(
            Button::new(SharedString::from(format!("composer-attachment-remove-{remove_id}")))
                .ghost()
                .small()
                .label("Remove")
                .on_click(cx.listener(move |this, _, _, cx| this.remove(&remove_id, cx))),
        )
}

fn format_size(size_bytes: u64) -> String {
    if size_bytes >= 1024 * 1024 {
        format!("{:.1} MiB", size_bytes as f64 / (1024. * 1024.))
    } else {
        format!("{} KiB", size_bytes.div_ceil(1024).max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt as _;
    use std::{cell::RefCell, rc::Rc};

    struct TempFiles(Vec<PathBuf>);

    impl TempFiles {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("t3-attachment-test-{}", t3_client::new_id()));
            std::fs::create_dir_all(&root).unwrap();
            let paths = ["first.txt", "pending.txt", "newer.txt"]
                .into_iter()
                .map(|name| root.join(name))
                .collect::<Vec<_>>();
            for path in &paths {
                std::fs::write(path, b"fixture").unwrap();
            }
            Self(paths)
        }
    }

    impl Drop for TempFiles {
        fn drop(&mut self) {
            if let Some(parent) = self.0.first().and_then(|path| path.parent()) {
                let _ = std::fs::remove_dir_all(parent);
            }
        }
    }

    fn panel(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<AttachmentPanel>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(700.), px(500.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(AttachmentPanel::new),
            )
            .unwrap()
        })
    }

    #[gpui_kit::test]
    fn upload_retry_ignores_stale_completion_and_send_clear_keeps_new_files(
        cx: &mut TestAppContext,
    ) {
        let files = TempFiles::new();
        let (handle, panel) = panel(cx);
        let events = Rc::new(RefCell::new(Vec::new()));
        let capture = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&panel, move |_, event: &AttachmentPanelEvent, _| {
                capture.borrow_mut().push(event.clone());
            })
        });

        cx.update_window(handle, |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.set_connected(true, cx);
                panel.add_paths(files.0[..2].iter().cloned(), cx);
            });
            let (first, pending) = {
                let panel = panel.read(cx);
                let rows = panel.rows();
                (rows[0].clone(), rows[1].clone())
            };
            panel.update(cx, |panel, cx| {
                panel.complete_upload(
                    &first.attachment.id,
                    first.request_id,
                    Err("network interrupted".into()),
                    cx,
                );
            });
            window.render_frame(cx);
            let retry_button = format!("composer-attachment-retry-{}", first.attachment.id);
            window.click(SharedString::from(retry_button), cx);
            let retry_id = panel.read(cx).rows()[0].request_id;
            assert_ne!(retry_id, first.request_id);

            panel.update(cx, |panel, cx| {
                panel.complete_upload(
                    &first.attachment.id,
                    first.request_id,
                    Ok(UploadedAttachment {
                        kind: first.attachment.kind,
                        id: "stale-id".into(),
                        name: first.attachment.name.clone(),
                        mime_type: first.attachment.mime_type.clone(),
                        size_bytes: first.attachment.size_bytes,
                    }),
                    cx,
                );
            });
            assert!(matches!(&panel.read(cx).rows()[0].status, AttachmentStatus::Uploading));
            panel.update(cx, |panel, cx| {
                panel.complete_upload(
                    &first.attachment.id,
                    retry_id,
                    Ok(UploadedAttachment {
                        kind: first.attachment.kind,
                        id: "uploaded-first".into(),
                        name: first.attachment.name.clone(),
                        mime_type: first.attachment.mime_type.clone(),
                        size_bytes: first.attachment.size_bytes,
                    }),
                    cx,
                );
            });

            window.render_frame(cx);
            let remove_button = format!("composer-attachment-remove-{}", pending.attachment.id);
            window.click(SharedString::from(remove_button), cx);
            assert_eq!(panel.read(cx).rows().len(), 1);
            panel.update(cx, |panel, cx| panel.add_paths([files.0[2].clone()], cx));
            let sent_ids = vec!["uploaded-first".to_owned()];
            panel.update(cx, |panel, cx| panel.clear_sent(&sent_ids, cx));
            let rows = panel.read(cx).rows();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].attachment.path, files.0[2]);
            assert!(matches!(&rows[0].status, AttachmentStatus::Uploading));
        })
        .unwrap();
    }
}
