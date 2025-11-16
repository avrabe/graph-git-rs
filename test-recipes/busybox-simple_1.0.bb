# Simplified BusyBox recipe for Hitzeleiter testing
# Tests: compilation, dependencies, Python blocks, task execution

SUMMARY = "BusyBox - minimal version for testing"
DESCRIPTION = "Simplified BusyBox recipe to test build system features"
LICENSE = "GPL-2.0-only"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/GPL-2.0-only;md5=801f80980d171dd6425610833a22dbe6"

# Test dependency handling
DEPENDS = "virtual/libc"
RDEPENDS:${PN} = ""

# Simulate source
SRC_URI = "file://busybox.c \
           file://busybox.h"

S = "${WORKDIR}"

# Python block to test Python execution
python __anonymous() {
    # Test variable manipulation
    pn = d.getVar('PN')
    pv = d.getVar('PV')
    d.setVar('FULL_NAME', '%s-%s' % (pn, pv))

    # Test conditional dependencies
    features = d.getVar('DISTRO_FEATURES') or ''
    if 'systemd' in features:
        d.appendVar('DEPENDS', ' systemd')
        d.setVar('INIT_SYSTEM', 'systemd')
    else:
        d.setVar('INIT_SYSTEM', 'sysvinit')

    # Test bb.utils.contains
    if bb.utils.contains('DISTRO_FEATURES', 'ipv6', True, False, d):
        d.setVar('IPV6_SUPPORT', '1')
}

# Test variable expansion with Python expressions
COMPILED_FLAGS = "${@d.getVar('CFLAGS') or '-O2'}"
BUILD_INFO = "${@'%s built on %s' % (d.getVar('PN'), d.getVar('DATETIME'))}"

do_configure() {
    echo "Configuring ${PN}-${PV}"
    echo "Full name: ${FULL_NAME}"
    echo "Init system: ${INIT_SYSTEM}"
    echo "IPv6 support: ${IPV6_SUPPORT}"
}

do_compile() {
    echo "Compiling busybox..."

    # Simulate compilation
    ${CC} ${CFLAGS} ${LDFLAGS} -c busybox.c -o busybox.o || echo "busybox.o"
    ${CC} ${LDFLAGS} busybox.o -o busybox || echo "busybox"

    echo "Compilation complete"
}

do_install() {
    echo "Installing to ${D}"

    install -d ${D}${bindir}
    install -d ${D}${sbindir}

    # Install busybox binary
    install -m 0755 busybox ${D}${bindir}/busybox || touch ${D}${bindir}/busybox

    # Create some symlinks
    ln -sf ${bindir}/busybox ${D}${bindir}/sh
    ln -sf ${bindir}/busybox ${D}${bindir}/ls
    ln -sf ${bindir}/busybox ${D}${sbindir}/init

    echo "Installation complete"
}

# Test package splitting
PACKAGES = "${PN} ${PN}-dev ${PN}-doc"

FILES:${PN} = "${bindir}/* ${sbindir}/*"
FILES:${PN}-dev = "${includedir}/*"
FILES:${PN}-doc = "${docdir}/*"
