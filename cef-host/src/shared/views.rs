use cef::*;
use std::cell::RefCell;

wrap_window_delegate! {
    pub struct HostWindowDelegate {
        browser_view: RefCell<Option<BrowserView>>,
        initial_show_state: ShowState,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 1024, height: 768 }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let browser_view = self.browser_view.borrow();
            let (Some(window), Some(browser_view)) = (window, browser_view.as_ref()) else {
                return;
            };
            let mut view = View::from(browser_view);
            window.add_child_view(Some(&mut view));

            if self.initial_show_state != ShowState::HIDDEN {
                window.show();
            }
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            *self.browser_view.borrow_mut() = None;
        }

        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
            let browser_view = self.browser_view.borrow();
            let browser_view = match browser_view.as_ref() {
                Some(bv) => bv,
                None => return 1,
            };
            if let Some(browser) = browser_view.browser() {
                if let Some(host) = browser.host() {
                    return host.try_close_browser();
                }
            }
            1
        }

        fn initial_show_state(&self, _window: Option<&mut Window>) -> ShowState {
            self.initial_show_state
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }
    }
}

wrap_browser_view_delegate! {
    pub struct HostBrowserViewDelegate {}

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn on_popup_browser_view_created(
            &self,
            _browser_view: Option<&mut BrowserView>,
            popup_browser_view: Option<&mut BrowserView>,
            _is_devtools: i32,
        ) -> i32 {
            // Wrap any popup (e.g. Google's 2FA challenge windows) in a top-level window
            // so the user can interact with it.
            let mut delegate = HostWindowDelegate::new(
                RefCell::new(popup_browser_view.cloned()),
                ShowState::NORMAL,
            );
            window_create_top_level(Some(&mut delegate));
            1
        }

        fn browser_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }
    }
}
