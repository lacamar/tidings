Name:           tidings
Version:        0.3.0
Release:        1%{?dist}
Summary:        Feed reader for Linux

License:        MIT
URL:            https://github.com/lacamar/%{name}
Source0:        %{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.gz

%global debug_package %{nil}

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc-c++
BuildRequires:  qt6-qtbase-devel
BuildRequires:  qt6-qtdeclarative-devel
BuildRequires:  qt6-qtwebengine-devel
BuildRequires:  desktop-file-utils
BuildRequires:  systemd-rpm-macros
Requires:       qt6-qtdeclarative
Requires:       qt6-qtwebengine

%description
RSS and Atom reader built with Qt Quick.

%prep
%autosetup -a1
mkdir -p .cargo
printf '[source.crates-io]\nreplace-with = "vendored-sources"\n\n[source.vendored-sources]\ndirectory = "vendor"\n' > .cargo/config.toml

%build
cargo build --release --offline --locked

%install
install -Dm755 target/release/%{name} %{buildroot}%{_bindir}/%{name}
install -Dm644 tidings.service %{buildroot}%{_userunitdir}/tidings.service
desktop-file-install --dir=%{buildroot}%{_datadir}/applications dev.lacamar.Tidings.desktop

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/dev.lacamar.Tidings.desktop
cargo test --release --offline --locked

%files
%license LICENSE
%{_bindir}/%{name}
%{_datadir}/applications/dev.lacamar.Tidings.desktop
%{_userunitdir}/tidings.service

%post
%systemd_user_post tidings.service

%preun
%systemd_user_preun tidings.service

%changelog
* Thu Nov 12 2026 lacamar <lchlnm@pm.me> - 0.3.0-1
- Rewrite in Rust (cxx-qt)
- Drop Python dependencies
- Smooth wheel scrolling, faster touchpad scrolling with fling
- Keep audio/video playing in background with mini player

* Thu Nov 12 2026 lacamar <lchlnm@pm.me> - 0.2.6-1
- Fix overlapping article rows after refresh

* Mon Nov 02 2026 lacamar <lchlnm@pm.me> - 0.2.5-1
- Send notifications as transient
- Ignore close errors for stale notifications

* Fri Oct 30 2026 lacamar <lchlnm@pm.me> - 0.2.4-1
- Reload theme on in-place rewrites

* Mon Oct 26 2026 lacamar <lchlnm@pm.me> - 0.2.3-1
- Lint fixes

* Mon Oct 26 2026 lacamar <lchlnm@pm.me> - 0.2.2-2
- Run all tests in %%check
* Mon Oct 19 2026 lacamar <lchlnm@pm.me> - 0.2.2-1
- Block ads and trackers in web viewer
- Block third-party cookies
- Hide cookie banners
- Send GPC/DNT headers
- Strip tracking params from links
- Save settings on SIGTERM

* Mon Oct 19 2026 lacamar <lchlnm@pm.me> - 0.2.1-1
- Follow system dark mode in web pages
- Add forced dark mode for web pages

* Mon Oct 19 2026 lacamar <lchlnm@pm.me> - 0.2.0-1
- Hide duplicate stories across feeds
- Mute filters
- Calm mode
- Auto-mark old unread as read

* Mon Oct 19 2026 lacamar <lchlnm@pm.me> - 0.1.3-1
- Custom colour theme from ~/.config/tidings/theme.json
- Translucent window background

* Mon Oct 19 2026 lacamar <lchlnm@pm.me> - 0.1.2-1
- Drop read articles from desktop notification

* Sun Oct 18 2026 lacamar <lchlnm@pm.me> - 0.1.1-1
- Add systemd user service
- Autostart toggle uses the service

* Sat Oct 17 2026 lacamar <lchlnm@pm.me> - 0.1.0-1
- Initial package
