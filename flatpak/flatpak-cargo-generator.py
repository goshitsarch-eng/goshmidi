#!/usr/bin/env python3
"""Generate offline, checksum-pinned registry sources from the locked dependencies.
Only crates.io packages are accepted; Git dependencies require an explicit audit.
"""
import argparse,json,pathlib,tomllib

def generate(lock):
 sources=[]
 for package in sorted(tomllib.loads(pathlib.Path(lock).read_text())['package'],key=lambda p:(p['name'],p['version'])):
  if 'source' not in package:continue
  if package['source']!='registry+https://github.com/rust-lang/crates.io-index':raise ValueError('Unaudited dependency source: '+package['source'])
  name=package['name'];version=package['version'];checksum=package['checksum'];destination=f'cargo/vendor/{name}-{version}'
  sources.append({'type':'archive','archive-type':'tar-gzip','url':f'https://static.crates.io/crates/{name}/{name}-{version}.crate','sha256':checksum,'dest':destination})
  sources.append({'type':'inline','contents':json.dumps({'package':checksum,'files':{}}),'dest':destination,'dest-filename':'.cargo-checksum.json'})
 sources.append({'type':'inline','contents':'[source.vendored-sources]\ndirectory = "cargo/vendor"\n\n[source.crates-io]\nreplace-with = "vendored-sources"\n','dest':'cargo','dest-filename':'config.toml'})
 return sources
if __name__=='__main__':
 parser=argparse.ArgumentParser();parser.add_argument('lock');parser.add_argument('-o','--output',required=True);args=parser.parse_args();pathlib.Path(args.output).write_text(json.dumps(generate(args.lock),indent=2)+'\n')
