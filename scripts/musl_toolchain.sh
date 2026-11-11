#!/usr/bin/env bash
# Puts musl-gcc on PATH for ring's C in the musl CLI builds (ci.yml
# cli-matrix, release.yml cli). Ubuntu's musl-tools first, under a time
# limit; when its mirrors cannot be reached (2026-11-11: runs failed
# resolving azure.archive.ubuntu.com and ports.ubuntu.com), a musl-gcc
# wrapper over Zig's clang and bundled musl, from the ziglang 0.15.2 wheel
# on PyPI, checked against its SHA-256 (pypi.org/pypi/ziglang/0.15.2/json,
# fetched 2026-11-11). Either way the job's CC_<target>=musl-gcc and its
# cargo build stay as they are.
set -euo pipefail

apt=(-o Acquire::Retries=2 -o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20)
if timeout 240 sudo apt-get "${apt[@]}" update \
  && timeout 240 sudo apt-get "${apt[@]}" install -y musl-tools; then
  exit 0
fi
echo "::warning::musl-tools unavailable from Ubuntu's mirrors; using Zig's musl C compiler"

pypi=https://files.pythonhosted.org/packages
case "$(uname -m)" in
  x86_64)
    url=$pypi/db/81/f9f5c7fd68d80b8edff2a641a1c95ac4125356387d298374227040b8e1e8/ziglang-0.15.2-py3-none-manylinux_2_12_x86_64.manylinux2010_x86_64.musllinux_1_1_x86_64.whl
    sha=806316453d1ede7174ed3fefb8e5348154629089c3bb5fe812dfd0e172fac130 ;;
  aarch64)
    url=$pypi/53/7d/8c277208250ffa72f12a10f52dfc1d45850f08244093065b40c5f4628260/ziglang-0.15.2-py3-none-manylinux_2_17_aarch64.manylinux2014_aarch64.musllinux_1_1_aarch64.whl
    sha=edc0aa60ec964a4cf462d40f68d7de242ddf37fd9a80f2afaee6397059463230 ;;
  *) echo "no pinned Zig for $(uname -m)" >&2; exit 1 ;;
esac
dir="${RUNNER_TEMP:-$PWD/.tmp}/musl-zig"
mkdir -p "$dir"
curl -fsSL --retry 3 -o "$dir/zig.whl" "$url"
echo "$sha  $dir/zig.whl" | sha256sum -c -
python3 -I -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])' "$dir/zig.whl" "$dir"
chmod +x "$dir/ziglang/zig"
# cc counts zig cc as clang and passes --target=<Rust triple>, which zig
# does not parse; the wrapper drops it and names the runner's own musl.
cat > "$dir/musl-gcc" <<WRAPPER
#!/usr/bin/env bash
args=()
for a in "\$@"; do case "\$a" in --target=*) ;; *) args+=("\$a") ;; esac; done
exec "$dir/ziglang/zig" cc -target "$(uname -m)-linux-musl" "\${args[@]}"
WRAPPER
chmod +x "$dir/musl-gcc"
"$dir/musl-gcc" --version
echo "$dir" >> "${GITHUB_PATH:-/dev/null}"
