# ADR-001 runtime experiment

Windows x64 only. This compares two implementations of a small synthetic mail
cache workload. It is not the mail-hantong product and cannot send mail or
access accounts. The product core and CLI now use Rust by decision in
[ADR-001](../../docs/adr/ADR-001-core-runtime.md); database, IPC, GUI and individual
library decisions remain separate. Both prototypes and their original results
are retained as comparison evidence.

See [SPEC.md](SPEC.md) for exact behavior and measurement boundaries, and the
[current measurement report](../../docs/latest/RUNTIME-BENCHMARK.md) for results.

## Reproduce

Prerequisites: Windows x64, PowerShell, Python 3.10+, Rust with the MSVC target
(the recorded run uses rustc/cargo 1.92.0), and Visual Studio 2022 or later with
the Desktop development with C++ workload and Windows SDK. Do not run other
builds, tests or benchmarks concurrently with measurement.

From the repository root:

```powershell
./experiments/runtime/build.ps1
python -m unittest discover -s experiments/runtime -p 'test_*.py' -v
python experiments/runtime/benchmark.py --rounds 40 --warmups 3 --environment experiments/runtime/results/build-manifest.json --output experiments/runtime/results/my-run.json
```

The build script downloads the pinned .NET SDK 10.0.403 Windows x64 archive
from Microsoft if needed, verifies its SHA-512, and extracts it into `.tools/`.
Native AOT tool packages restore into `.tools/nuget/`. It does not install a
system SDK or change the global PATH. These are build-time downloads; the
experiment uses only local generated data. Rust has no external crates.

The build manifest records tool versions, commands and source hashes. The
runner requires this manifest, preserves existing result files, records binary
and fixture hashes, checks every response, and retains warmup and measured raw
samples. Use a new output filename for each run. Rebuild after source changes.
Generated fixture data and SDK/build directories are ignored by Git.

## Measurements

- Process creation through `--ping` response and process exit.
- Fresh process creation through parsing/sorting 100,000 rows and `READY`.
- Resident list, English/Korean subject search, snippet search, missing query,
  and 128 KiB body file read/checksum, including Python/Windows pipe overhead.
- Live working set, peak working set and private bytes at READY and after the
  command sequence; Windows process CPU time; executable bytes excluding PDBs.

Each round uses a new resident process per candidate and alternates candidate
order. The default report run has 40 rounds and 3 warmup rounds. Resident
metrics have 5 account-specific requests per round, so 200 recorded requests
are **not 200 independent process samples**. Warm OS file cache is intentional;
disk-cold startup is not measured. Percentiles use nearest rank. Results from
different processes or runs may vary with scheduling, antivirus and power state.

Both implementations own all record text fields plus ID/account/UTC/unread.
Rust uses a vector of structs; C# uses a list of readonly record structs. Runtime
allocation, UTF-8/UTF-16 representations and standard-library parsing/search
remain different. This is a comparison of these implementations, not a claim
that one language is universally faster. .NET invariant globalization is used
for this ordinal-search experiment, not adopted as a product localization policy.

## Not covered

Authenticated product IPC, policy/approval checks, real DB indexes, MIME parsing,
provider SDKs, GUI rendering, synchronization, sustained concurrency, and macOS
are absent. Therefore the product's 1-second usable-screen target and OPS-01
acceptance criteria are not verified. Tests have OPS-01 names solely to identify
the motivating requirement; do not count them as product acceptance coverage.

Official build references: [Native AOT prerequisites](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/),
[Rust release profiles](https://doc.rust-lang.org/cargo/reference/profiles.html).
