param([string]$Root='<AUDIT_ROOT>')
$env:TEMP="$Root\tmp"; $env:TMP=$env:TEMP; $env:CARGO_HOME="$Root\cargo-home"; $env:CARGO_TARGET_DIR="$Root\num-target-c"; $env:PYTHONDONTWRITEBYTECODE='1'; $env:RAYON_NUM_THREADS='1'
$ev="$Root\repo\docs\verification\2026-11-28-soneflow-0.2.1-acceptance-ddbe9e2\numerical"
$source="$Root\num-c\soneflow\src\core\main_loudness\mod.rs"
$original=[IO.File]::ReadAllBytes($source)
$text=[IO.File]::ReadAllText($source)
$pattern='(?s)let mp1_table = match read_rounding_environment\(\) \{.*?\n    \};'
$mutant=[regex]::Replace($text,$pattern,'let mp1_table = Some(mp1_table_rn());')
if($text -eq $mutant){throw 'mutation failed'}
try {
 [IO.File]::WriteAllText($source,$mutant)
 & cargo test --offline --manifest-path "$Root\num-c\Cargo.toml" -p soneflow --features internals --test main_loudness_rounding -- --nocapture 2>&1 | Out-File "$ev\mutation-debug.log"
 $code=$LASTEXITCODE
 "mutation exit=$code" | Out-File "$ev\mutation-status.txt"
 if($code -eq 0){throw 'mutation survived'}
} finally {[IO.File]::WriteAllBytes($source,$original)}
foreach($profile in 'debug','release'){
 $opts=@();if($profile -eq 'release'){$opts=@('--release')}
 & cargo test --offline --manifest-path "$Root\num-c\Cargo.toml" -p soneflow --features internals --test main_loudness_rounding @opts -- --nocapture 2>&1 | Out-File "$ev\restored-$profile.log"
 if($LASTEXITCODE -ne 0){throw "restored failure $profile"}
}
$core="$Root\num-c\soneflow\src\core\main_loudness\core_band.rs"
$coreOriginal=[IO.File]::ReadAllBytes($core)
try{
 [IO.File]::AppendAllText($core,"`npub(super) fn acceptance_cache_ready() -> bool { MP1_TABLE_RN.get().is_some() }`n")
 [IO.File]::AppendAllText($source,"`npub fn acceptance_cache_ready() -> bool { core_band::acceptance_cache_ready() }`n")
 Copy-Item "$ev\cache_probe.rs" "$Root\num-c\soneflow\examples\cache_probe.rs"
 & cargo build --offline --manifest-path "$Root\num-c\Cargo.toml" -p soneflow --features internals --example cache_probe --release 2>&1 | Out-File "$ev\cache-build.log"
 if($LASTEXITCODE -ne 0){throw 'cache build failed'}
 foreach($bits in 0,64,32768,32832){
  & "$env:CARGO_TARGET_DIR\release\examples\cache_probe.exe" $bits > "$ev\cache-$bits.log"
  if($LASTEXITCODE -ne 0){throw 'cache test failed'}
 }
}finally{[IO.File]::WriteAllBytes($source,$original);[IO.File]::WriteAllBytes($core,$coreOriginal)}
Get-FileHash $source,$core | Format-List | Out-File "$ev\restored-source-sha256.txt"
Get-FileHash "$Root\repo\soneflow\src\core\main_loudness\mod.rs","$Root\repo\soneflow\src\core\main_loudness\core_band.rs" | Format-List | Out-File "$ev\original-source-sha256.txt"
