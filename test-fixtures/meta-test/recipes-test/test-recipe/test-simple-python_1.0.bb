# Simplified test recipe for Python IR validation
# Tests basic patterns without override complexity

SUMMARY = "Simple test for Python block processing"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

# Base dependencies - should be found without Python
DEPENDS = "base-dep"
RDEPENDS = "runtime-dep"

# Test 1: Python block that adds to DEPENDS directly
python __anonymous() {
    d.appendVar('DEPENDS', ' python-added-dep')
}

# Test 2: Python block with bb.utils.contains that adds to RDEPENDS
python __anonymous() {
    if bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d):
        d.appendVar('RDEPENDS', ' systemd')
}

# Test 3: Python block that sets a variable based on condition
python __anonymous() {
    features = d.getVar('DISTRO_FEATURES', True) or ''
    if 'systemd' in features:
        d.setVar('HAS_SYSTEMD', '1')
        d.appendVar('DEPENDS', ' systemd-dep')
    else:
        d.setVar('HAS_SYSTEMD', '0')
}
