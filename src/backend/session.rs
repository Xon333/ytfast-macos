use super::*;

/// Authentication is staged without changing the active client or resolver.
/// Only the worker can publish the result of its newest connection attempt.
pub(super) struct Connection {
    epoch: u64,
    account: Account,
    session: Option<crate::auth::Session>,
    profiles: Vec<crate::auth::Profile>,
    current: Option<String>,
}

/// Each published session has an immutable private export. Replacing an
/// account cannot change the cookies underneath an older resolver request.
pub(super) struct CookieFile {
    path: std::path::PathBuf,
    scope: String,
}

impl Drop for CookieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl super::Worker {
    pub(super) fn connect(&mut self) {
        self.last_connect = Some(Instant::now());
        self.connect_epoch = self.connect_epoch.wrapping_add(1);
        let epoch = self.connect_epoch;
        if let Some(previous) = self.connecting.take() {
            previous.abort();
        }
        // Invalidate old reads and queued writes immediately. Already playing
        // audio can continue while the selected browser is checked.
        self.client.set_session(None);
        self.sink.send(Event::Account(Account::Checking));
        let client = self.client.snapshot();
        let paths = self.paths.clone();
        let tx = self.internal_tx.clone();
        let task = tokio::spawn(async move {
            let preferred = crate::settings::Settings::load(&paths).browser_profile;
            let selected = preferred.clone();
            let loaded = tokio::task::spawn_blocking(move || {
                crate::auth::load_with_profiles(selected.as_deref())
            })
            .await;
            let (session, profiles) = match loaded {
                Ok(loaded) => loaded,
                Err(error) => (Err(anyhow::anyhow!(error)), Vec::new()),
            };
            let current = session
                .as_ref()
                .ok()
                .map(|session| session.profile.clone())
                .or(preferred);
            let mut connection = Connection {
                epoch,
                account: Account::Checking,
                session: None,
                profiles,
                current,
            };
            match session {
                Ok(session) => {
                    let source = session.source.clone();
                    client.set_session(Some(session.clone()));
                    connection.account = match client.account().await {
                        Ok(value) => match parse::account(&value) {
                            Some((name, photo)) => {
                                connection.session = Some(session);
                                Account::SignedIn {
                                    name,
                                    photo,
                                    source,
                                }
                            }
                            None => Account::SignedOut {
                                reason: format!("{source} isn't signed in to YouTube Music"),
                            },
                        },
                        Err(ApiError::Auth) => Account::SignedOut {
                            reason: format!("The YouTube session in {source} has expired"),
                        },
                        Err(error) => {
                            // Being offline does not mean the browser signed out.
                            connection.session = Some(session);
                            Account::Unverified {
                                reason: error.to_string(),
                            }
                        }
                    };
                }
                Err(error) => {
                    connection.account = Account::SignedOut {
                        reason: format!("{error:#}"),
                    };
                }
            }
            let _ = tx.send(Internal::Connected(connection));
        });
        self.connecting = Some(task.abort_handle());
    }

    pub(super) async fn connected(&mut self, mut connection: Connection) {
        if connection.epoch != self.connect_epoch {
            return;
        }
        self.connecting = None;
        let scope = connection
            .session
            .as_ref()
            .map(crate::auth::Session::cache_scope);
        if self.session_cookie.as_ref().map(|cookie| &cookie.scope) != scope.as_ref() {
            self.account_changed().await;
        }
        if let Some(session) = connection.session.take() {
            let cookie_path = self
                .paths
                .runtime
                .join(format!("cookies-{:032x}.txt", fastrand::u128(..)));
            match session.write_netscape(&cookie_path) {
                Ok(()) => {
                    self.resolver
                        .set_cookie_file(Some(cookie_path.clone()), Some(session.cache_scope()));
                    self.session_cookie = Some(CookieFile {
                        path: cookie_path,
                        scope: session.cache_scope(),
                    });
                    let mut settings = crate::settings::Settings::load(&self.paths);
                    if settings.browser_profile.is_none() {
                        // Remember the initially discovered profile. Reconnect
                        // must not drift to a recently used different browser.
                        settings.browser_profile = Some(session.profile.clone());
                        if let Err(error) = settings.save(&self.paths) {
                            self.sink.send(Event::Error(format!(
                                "Couldn't remember the browser profile: {error}"
                            )));
                        }
                    }
                    self.client.set_session(Some(session));
                }
                Err(error) => {
                    self.resolver.set_cookie_file(None, None);
                    self.session_cookie = None;
                    connection.account = Account::Unverified {
                        reason: format!("Couldn't prepare authenticated playback: {error}"),
                    };
                }
            }
        } else {
            self.resolver.set_cookie_file(None, None);
            self.session_cookie = None;
        }
        // Remove the older fixed-name export once a connection was handled.
        let _ = std::fs::remove_file(self.paths.cookie_file());
        self.sink.send(Event::Profiles {
            list: connection.profiles,
            current: connection.current,
        });
        self.sink.send(Event::Account(connection.account));
        self.prepare_restored();
    }

    pub(super) fn auth_failed(&mut self, epoch: u64) {
        if epoch == self.client.session_epoch()
            && self.client.signed_in()
            && self
                .last_connect
                .is_none_or(|t| t.elapsed() > Duration::from_secs(60))
        {
            self.connect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_older_connection_cannot_publish_after_a_newer_choice() {
        let root = std::env::temp_dir().join(format!("ytfast-connect-{:032x}", fastrand::u128(..)));
        let paths = Paths {
            config: root.join("config"),
            cache: root.join("cache"),
            runtime: root.join("runtime"),
        };
        for path in [&paths.config, &paths.cache, &paths.runtime] {
            crate::paths::private_dir(path).unwrap();
        }
        let (tx, events) = std::sync::mpsc::channel();
        let (now, _) = tokio::sync::watch::channel(crate::desktop::Now::default());
        let sink = Sink {
            tx,
            wake: Arc::new(|| {}),
            now: Arc::new(now),
        };
        let client = Arc::new(Client::new());
        let resolver = Arc::new(Resolver::new(paths.runtime.clone()));
        let mut worker = Worker::new(client.clone(), resolver, paths, sink);
        worker.connect_epoch = 2;
        let result = |epoch, reason: &str| Connection {
            epoch,
            account: Account::SignedOut {
                reason: reason.into(),
            },
            session: None,
            profiles: vec![],
            current: Some(reason.into()),
        };
        let before = client.session_epoch();
        worker.connected(result(1, "stale")).await;
        assert!(events.try_recv().is_err());
        assert_eq!(client.session_epoch(), before);
        worker.connected(result(2, "selected")).await;
        assert!(
            matches!(events.recv().unwrap(), Event::Profiles { current: Some(current), .. } if current == "selected")
        );
        assert!(
            matches!(events.recv().unwrap(), Event::Account(Account::SignedOut { reason }) if reason == "selected")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
