#!/usr/bin/env python3
"""Fetch pinned redistributable resources; verify before replacing any file."""
import hashlib, pathlib, ssl, urllib.request
ROOT=pathlib.Path(__file__).resolve().parents[1]
RESOURCES={
 'FluidR3Mono_GM.sf3':('https://raw.githubusercontent.com/LibreScore/sf3/3a6eda823dac2ec3e134d38544ca839217358f08/FluidR3Mono_GM.sf3.wasm','cfcd66d89e8386823400eca64934b14fbea7bf48ba1f00d21189af1262794ec2'),
 'FluidR3Mono_License.md':('https://raw.githubusercontent.com/LibreScore/sf3/3a6eda823dac2ec3e134d38544ca839217358f08/FluidR3Mono_License.md','c21ed6be2fea862673953977c616befc28d1b20dfa60349e4461b97c759db6ca'),
}
def fetch(destination):
 destination.mkdir(parents=True,exist_ok=True)
 for name,(url,digest) in RESOURCES.items():
  path=destination/name
  if path.exists() and hashlib.sha256(path.read_bytes()).hexdigest()==digest:continue
  with urllib.request.urlopen(url,timeout=90,context=ssl.create_default_context()) as response:data=response.read(32*1024*1024)
  if hashlib.sha256(data).hexdigest()!=digest:raise RuntimeError('Resource checksum mismatch: '+name)
  temporary=path.with_suffix(path.suffix+'.part');temporary.write_bytes(data);temporary.replace(path)
  print('Verified '+name)
if __name__=='__main__':fetch(ROOT/'packaging/resources/soundfonts')
