# Test recipe with complex Python patterns
# Tests edge cases and patterns that require RustPython

SUMMARY = "Test recipe with complex Python patterns"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

DEPENDS = "base-package"

# Test 6: Complex Python with loop (requires RustPython)
python __anonymous() {
    packages = d.getVar('PACKAGES', True) or ''
    for pkg in packages.split():
        d.setVar('FILES_' + pkg, '/usr/bin/' + pkg)
}

# Test 7: Complex Python with multiple conditions
python __anonymous() {
    features = d.getVar('DISTRO_FEATURES', True) or ''
    deps = []

    if 'systemd' in features:
        deps.append('systemd')
    if 'wayland' in features:
        deps.append('wayland')
    if 'x11' in features:
        deps.append('libx11')

    if deps:
        d.appendVar('RDEPENDS:${PN}', ' ' + ' '.join(deps))
}

# Test 8: String concatenation and formatting
python __anonymous() {
    pn = d.getVar('PN', True)
    pv = d.getVar('PV', True)
    d.setVar('FULL_NAME', pn + '-' + pv)
}

# Test 9: getVar with expansion
python __anonymous() {
    workdir = d.getVar('WORKDIR', True)
    d.setVar('BUILD_DIR', workdir + '/build')
}

# Test 10: Contains with multiple checks
python __anonymous() {
    result = 'none'
    if bb.utils.contains('MACHINE_FEATURES', 'wifi', True, False, d):
        result = 'wifi'
    if bb.utils.contains('MACHINE_FEATURES', 'bluetooth', True, False, d):
        if result != 'none':
            result = result + '-bluetooth'
        else:
            result = 'bluetooth'
    d.setVar('CONNECTIVITY', result)
}
