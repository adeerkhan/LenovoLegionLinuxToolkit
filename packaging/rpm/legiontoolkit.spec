%global debug_package %{nil}

Name:           legiontoolkit
Version:        0.1.0
Release:        1%{?dist}
Summary:        Native control panel for Lenovo Legion laptops

License:        GPL-3.0-or-later
URL:            https://github.com/adeerkhan/LenovoLegionLinuxToolkit
Source0:        %{name}-%{version}.tar.gz

# The GUI is GTK 4.22 / libadwaita 1.9 (Fedora 44 and newer). The privileged
# helper is built without GTK on purpose: it runs as root and must not pull a
# GUI runtime into that process.
BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  gtk4-devel
BuildRequires:  libadwaita-devel
BuildRequires:  pango-devel
BuildRequires:  gdk-pixbuf2-devel
BuildRequires:  cairo-gobject-devel
BuildRequires:  graphene-devel

Requires:       polkit
Requires:       gtk4
Requires:       libadwaita

%description
A GTK 4 / libadwaita control panel for Lenovo Legion laptops: power modes,
thermals and fan speed, GPU working mode, lighting, battery charging mode, and
automation. Reads are unprivileged; each write escalates through polkit to a
small helper binary, so the application itself never runs as root.

%prep
%setup -q

%build
# The Makefile builds the GUI and the helper separately so the helper never
# links GTK; it also bakes HELPER_PATH in, which is why the prefix is passed
# through rather than assumed.
export PATH="%{_bindir}:$HOME/.cargo/bin:$PATH"
%make_build PREFIX=%{_prefix}

%install
export PATH="%{_bindir}:$HOME/.cargo/bin:$PATH"
# The binaries are already built; stage them plus the polkit policy, desktop
# entry and icons into the build root.
DESTDIR=%{buildroot} PREFIX=%{_prefix} ./install.sh install

%files
%{_bindir}/legiontoolkit
%{_bindir}/legiontoolkit-keyboard-probe
%{_libexecdir}/legiontoolkit-helper
%{_datadir}/applications/legiontoolkit.desktop
%{_datadir}/icons/hicolor/*/apps/legiontoolkit.png
%{_datadir}/polkit-1/actions/org.legiontoolkit.helper.policy
%license LICENSE
%doc README.md
%doc docs

%changelog
* Wed Oct 14 2026 Adeer Khan <adeerkhan@users.noreply.github.com> - 0.1.0-1
- Initial package
