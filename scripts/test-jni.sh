#!/usr/bin/env bash
set -euo pipefail
atn_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$atn_root"
cargo build -p atn-adapter --locked "${@}"
atn_classes="$(mktemp -d /tmp/atn-jni-smoke.XXXXXX)"
trap 'rm -rf "$atn_classes"' EXIT
javac -d "$atn_classes" android/app/src/main/java/org/freetak/atn/NativeCore.java crates/atn-adapter/tests/jni/NativeSmoke.java
java -Djava.library.path="$atn_root/target/debug" -cp "$atn_classes" NativeSmoke "$atn_root/examples/synthetic-config.json"
