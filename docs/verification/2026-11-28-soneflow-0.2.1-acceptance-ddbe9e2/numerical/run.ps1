param([string]$Root='<AUDIT_ROOT>')
$env:TEMP="$Root\tmp"; $env:TMP=$env:TEMP; $env:CARGO_HOME="$Root\cargo-home"; $env:PYTHONDONTWRITEBYTECODE='1'; $env:RAYON_NUM_THREADS='1'
$ev="$Root\repo\docs\verification\2026-11-28-soneflow-0.2.1-acceptance-ddbe9e2\numerical"
foreach($snap in 'b','c') {
  $env:CARGO_TARGET_DIR="$Root\num-target-c"
  foreach($profile in 'debug','release') {
    $opts=@();if($profile -eq 'release'){$opts=@('--release')}
    & cargo build --offline --manifest-path "$Root\num-$snap\Cargo.toml" -p soneflow --features internals --example acceptance_probe @opts 2>&1 | Out-File "$ev\build-$snap-$profile.log"
    if($LASTEXITCODE -ne 0){throw "build failure $snap $profile"}
    foreach($mode in 'rn','nonrn') {
      & "$env:CARGO_TARGET_DIR\$profile\examples\acceptance_probe.exe" $mode > "$ev\$snap-$profile-$mode.csv"
      if($LASTEXITCODE -ne 0){throw "probe failure $snap $profile $mode"}
    }
    if($snap -eq 'c') {
      foreach($bits in 0,64,32768,32832) {
        & "$env:CARGO_TARGET_DIR\$profile\examples\acceptance_probe.exe" "ftz$bits" > "$ev\c-$profile-ftz$bits.csv"
        if($LASTEXITCODE -ne 0){throw 'FTZ failure'}
      }
    }
  }
}

