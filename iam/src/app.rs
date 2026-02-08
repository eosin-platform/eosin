use std::{ops::Deref, sync::Arc};

pub struct AppInner {
    pub kc: eosin_common::args::KeycloakArgs,
}

#[derive(Clone)]
pub struct App {
    inner: Arc<AppInner>,
}

impl Deref for App {
    type Target = AppInner;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl App {
    pub fn new(kc: eosin_common::args::KeycloakArgs) -> Self {
        Self {
            inner: Arc::new(AppInner { kc }),
        }
    }
}
