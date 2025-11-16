# BusyBox - The Swiss Army Knife of Embedded Linux
# Simplified recipe for testing Hitzeleiter build system

SUMMARY = "Tiny versions of many common UNIX utilities in a single small executable"
DESCRIPTION = "BusyBox combines tiny versions of many common UNIX utilities into a single \
small executable. It provides minimalist replacements for most of the utilities you \
usually find in GNU coreutils, util-linux, etc."
HOMEPAGE = "https://www.busybox.net"
BUGTRACKER = "https://bugs.busybox.net/"

LICENSE = "GPL-2.0-only"
LIC_FILES_CHKSUM = "file://LICENSE;md5=de10de48642ab74318e893a61105afbb"

# Dependencies
DEPENDS = "virtual/kernel"
RDEPENDS:${PN} = ""

# Source
SRC_URI = "https://busybox.net/downloads/busybox-${PV}.tar.bz2 \
           file://defconfig \
           file://busybox-udhcpc-no_deconfig.patch"

SRC_URI[sha256sum] = "b8cc24c9574d809e7279c3be349795c5d5ceb6fdf19ca709f80cde50e47de314"

S = "${WORKDIR}/busybox-${PV}"

# Configuration
EXTRA_OEMAKE = "CROSS_COMPILE=${TARGET_PREFIX} SKIP_STRIP=y"

# Features - conditional compilation based on distro features
BUSYBOX_SPLIT_SUID ?= "1"

python __anonymous() {
    # Add systemd support if available
    if bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d):
        d.appendVar('DEPENDS', ' systemd')
        d.setVar('BUSYBOX_INIT_SYSTEM', 'systemd')
    else:
        d.setVar('BUSYBOX_INIT_SYSTEM', 'sysvinit')

    # Add mdev support for device management
    if bb.utils.contains('DISTRO_FEATURES', 'mdev', True, False, d):
        d.setVar('BUSYBOX_MDEV', '1')

    # Conditional RDEPENDS based on split suid
    if d.getVar('BUSYBOX_SPLIT_SUID') == '1':
        d.appendVar('RDEPENDS:${PN}', ' busybox-suid')
}

# BitBake tasks
do_configure() {
    # Install defconfig
    install -m 0644 ${WORKDIR}/defconfig ${S}/.config

    # Merge additional fragments if present
    if [ -f ${WORKDIR}/fragment.cfg ]; then
        cat ${WORKDIR}/fragment.cfg >> ${S}/.config
    fi

    # Run oldconfig to expand config
    oe_runmake oldconfig
}

do_compile() {
    # Build busybox
    unset CFLAGS CPPFLAGS CXXFLAGS LDFLAGS
    oe_runmake busybox_unstripped

    # Create links for all applets
    oe_runmake busybox.links
}

do_install() {
    # Install busybox binary
    install -d ${D}${base_bindir}
    install -d ${D}${bindir}
    install -d ${D}${sbindir}
    install -d ${D}${base_sbindir}

    # Install the busybox binary
    install -m 0755 busybox ${D}${base_bindir}/busybox

    # Create symlinks for all applets
    for app in $(cat busybox.links); do
        install -d ${D}$(dirname $app)
        ln -sf ${base_bindir}/busybox ${D}$app
    done

    # Install suid applets separately if split
    if [ "${BUSYBOX_SPLIT_SUID}" = "1" ]; then
        install -d ${D}${base_bindir}/busybox-suid
        # This would install suid applets separately
    fi

    # Install init scripts based on init system
    if [ "${BUSYBOX_INIT_SYSTEM}" = "systemd" ]; then
        install -d ${D}${systemd_system_unitdir}
        # Install systemd units here
    else
        install -d ${D}${sysconfdir}/init.d
        # Install sysvinit scripts here
    fi
}

# Package split
PACKAGES =+ "${PN}-httpd ${PN}-udhcpd ${PN}-udhcpc ${PN}-syslog ${PN}-mdev"

# File assignments for split packages
FILES:${PN}-httpd = "${bindir}/httpd"
FILES:${PN}-udhcpd = "${sbindir}/udhcpd ${sysconfdir}/udhcpd.conf"
FILES:${PN}-udhcpc = "${base_sbindir}/udhcpc ${datadir}/udhcpc"
FILES:${PN}-syslog = "${base_sbindir}/syslogd ${base_sbindir}/klogd"
FILES:${PN}-mdev = "${base_sbindir}/mdev"

# Runtime dependencies for subpackages
RDEPENDS:${PN}-udhcpc = "busybox"

# Allow empty packages (some may not be built depending on config)
ALLOW_EMPTY:${PN}-httpd = "1"
ALLOW_EMPTY:${PN}-udhcpd = "1"
ALLOW_EMPTY:${PN}-udhcpc = "1"
ALLOW_EMPTY:${PN}-syslog = "1"
ALLOW_EMPTY:${PN}-mdev = "1"

# QA checks
INSANE_SKIP:${PN} = "already-stripped"
