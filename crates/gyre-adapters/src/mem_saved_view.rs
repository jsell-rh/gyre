//! In-memory implementation of SavedViewRepository.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_ports::saved_view::{SavedView, SavedViewRepository};
use std::sync::Mutex;

pub struct MemSavedViewRepository {
    views: Mutex<Vec<SavedView>>,
}

impl Default for MemSavedViewRepository {
    fn default() -> Self {
        Self {
            views: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl SavedViewRepository for MemSavedViewRepository {
    async fn create(&self, view: SavedView) -> Result<SavedView> {
        self.views.lock().unwrap().push(view.clone());
        Ok(view)
    }

    async fn get(&self, id: &Id) -> Result<Option<SavedView>> {
        Ok(self
            .views
            .lock()
            .unwrap()
            .iter()
            .find(|v| v.id == *id)
            .cloned())
    }

    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<SavedView>> {
        Ok(self
            .views
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v.repo_id == *repo_id)
            .cloned()
            .collect())
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<SavedView>> {
        Ok(self
            .views
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v.workspace_id == *workspace_id)
            .cloned()
            .collect())
    }

    async fn update(&self, view: SavedView) -> Result<SavedView> {
        let mut views = self.views.lock().unwrap();
        if let Some(existing) = views.iter_mut().find(|v| v.id == view.id) {
            *existing = view.clone();
        }
        Ok(view)
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.views.lock().unwrap().retain(|v| v.id != *id);
        Ok(())
    }
}
