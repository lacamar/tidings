Name:           tidings
Version:        0.1.1
Release:        1%{?dist}
Summary:        Feed reader for Linux

License:        MIT
URL:            https://github.com/lacamar/%{name}
Source0:        %{name}-%{version}.tar.gz

BuildArch:      noarch
BuildRequires:  desktop-file-utils
BuildRequires:  systemd-rpm-macros
BuildRequires:  python3
BuildRequires:  python3-feedparser
BuildRequires:  python3-jeepney
BuildRequires:  python3-lxml
BuildRequires:  python3-pyside6
Requires:       python3-feedparser
Requires:       python3-jeepney
Requires:       python3-lxml
Requires:       python3-pyside6

%description
RSS and Atom reader built with Qt Quick.

%prep
%autosetup

%build

%install
install -Dm755 tidings.py %{buildroot}%{_datadir}/%{name}/tidings.py
install -Dm644 Main.qml %{buildroot}%{_datadir}/%{name}/Main.qml
mkdir -p %{buildroot}%{_bindir}
ln -s ../share/%{name}/tidings.py %{buildroot}%{_bindir}/%{name}
install -Dm644 tidings.service %{buildroot}%{_userunitdir}/tidings.service
desktop-file-install --dir=%{buildroot}%{_datadir}/applications dev.lacamar.Tidings.desktop

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/dev.lacamar.Tidings.desktop
python3 test_tidings.py

%files
%license LICENSE
%{_bindir}/%{name}
%{_datadir}/%{name}/
%{_datadir}/applications/dev.lacamar.Tidings.desktop
%{_userunitdir}/tidings.service

%post
%systemd_user_post tidings.service

%preun
%systemd_user_preun tidings.service

%changelog
* Sun Oct 18 2026 lacamar <lchlnm@pm.me> - 0.1.1-1
- Add systemd user service
- Autostart toggle uses the service

* lø. okt. 17 2026 lacamar <lchlnm@pm.me> - 0.1.0-1
- Initial package
