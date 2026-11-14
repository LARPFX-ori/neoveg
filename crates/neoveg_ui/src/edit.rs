//! Ponte fina da UI para as operações de edição em `neoveg_engine::session`
//! (mantida para chamadas diretas da timeline/menus de contexto sem duplicar lógica).

use super::App;
use crate::model::*;
use std::collections::HashSet;

impl App {
    pub fn expand_groups(&self, ids: &HashSet<Id>) -> HashSet<Id> {
        self.session.expand_groups(ids)
    }

    pub fn copy_selection(&mut self) {
        self.session.copy_selection();
        self.sync_status_from_session();
    }

    pub fn cut_selection(&mut self) {
        self.session.cut_selection();
        self.sync_status_from_session();
    }

    pub fn paste(&mut self, insert: bool) {
        self.session.paste(insert);
        self.sync_after_session();
    }

    pub fn delete_selection(&mut self) {
        self.session.delete_selection();
        self.sync_after_session();
    }

    pub fn split_at_cursor(&mut self) {
        self.session.split_at_cursor();
        self.sync_after_session();
    }

    pub fn trim_to_selection(&mut self) {
        self.session.trim_to_selection();
        self.sync_after_session();
    }

    pub fn trim_selected_start_to_cursor(&mut self) {
        self.session.trim_selected_start_to_cursor();
        self.sync_after_session();
    }

    pub fn trim_selected_end_to_cursor(&mut self) {
        self.session.trim_selected_end_to_cursor();
        self.sync_after_session();
    }

    pub fn delete_time_range(&mut self) {
        self.session.delete_time_range();
        self.sync_after_session();
    }

    pub fn insert_time(&mut self) {
        self.session.insert_time();
        self.sync_after_session();
    }

    pub fn close_gaps(&mut self) {
        self.session.close_gaps();
        self.sync_after_session();
    }

    pub fn group_selected(&mut self) {
        self.session.group_selected();
        self.sync_after_session();
    }

    pub fn ungroup_selected(&mut self) {
        self.session.ungroup_selected();
        self.sync_after_session();
    }

    pub fn select_group(&mut self) {
        self.session.select_group();
    }

    pub fn insert_marker(&mut self) {
        self.session.insert_marker();
        self.sync_after_session();
    }

    pub fn insert_region(&mut self) {
        self.session.insert_region();
        self.sync_after_session();
    }

    pub fn toggle_envelope_for_selected(&mut self, audio_kind: EnvKind, video_kind: EnvKind) {
        self.session
            .toggle_envelope_for_selected(audio_kind, video_kind);
        self.sync_after_session();
    }

    pub fn remove_envelope(&mut self, kind: EnvKind) {
        self.session.remove_envelope(kind);
        self.sync_after_session();
    }

    pub fn toggle_reverse(&mut self) {
        self.session.toggle_reverse();
        self.sync_after_session();
    }

    pub fn freeze_frame(&mut self) {
        self.session.freeze_frame();
        self.sync_after_session();
    }

    pub fn delete_track(&mut self, ti: usize) {
        self.session.delete_track(ti);
        self.sync_after_session();
    }

    pub fn duplicate_track(&mut self, ti: usize) {
        self.session.duplicate_track(ti);
        self.sync_after_session();
    }

    pub fn move_track(&mut self, ti: usize, up: bool) {
        self.session.move_track(ti, up);
        self.sync_after_session();
    }

    pub fn remove_unused_media(&mut self) {
        let removed = self.session.remove_unused_media(Some(&self.media));
        for id in removed {
            self.thumbs.remove(&id);
        }
        self.sync_after_session();
    }
}
