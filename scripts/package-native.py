#!/usr/bin/env python3
"""Stage native resources, licenses and runtimes, then create OS-native artifacts."""
import argparse,hashlib,io,json,os,struct,pathlib,platform,plistlib,shutil,ssl,subprocess,tarfile,tomllib,urllib.request,zipfile
ROOT=pathlib.Path(__file__).resolve().parents[1]
def run(*args):subprocess.run(args,check=True)
def copy(source,destination):destination.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(source,destination)
def permissions(root):
 for path in [root,*root.rglob('*')]:
  if not path.is_symlink():path.chmod(0o755 if path.is_dir() or path.stat().st_mode&0o111 else 0o644)
def rust_notices(destination):
 cargo=pathlib.Path(os.environ.get('CARGO_HOME',str(pathlib.Path.home()/'.cargo')))
 registries=[*list((cargo/'registry/src').glob('*')),cargo/'vendor'];records=[]
 for package in tomllib.loads((ROOT/'Cargo.lock').read_text())['package']:
  if not package.get('source','').startswith('registry+'):continue
  identity=package['name']+'-'+package['version']
  source=next((registry/identity for registry in registries if (registry/identity).is_dir()),None)
  if source is None:raise RuntimeError('Missing license source cache; run cargo fetch --locked: '+identity)
  metadata=tomllib.loads((source/'Cargo.toml').read_text())['package'];notices=[]
  for path in source.rglob('*'):
   if not path.is_file() or len(path.relative_to(source).parts)>3:continue
   if path.name.lower().startswith(('license','licence','copying','copyright','notice')) or path.relative_to(source).as_posix()==metadata.get('license-file'):
    relative=path.relative_to(source);copy(path,destination/identity/relative);notices.append(relative.as_posix())
  records.append({'name':package['name'],'version':package['version'],'license':metadata.get('license'),'authors':metadata.get('authors',[]),'repository':metadata.get('repository'),'source_sha256':package.get('checksum'),'notices':notices})
 destination.mkdir(parents=True,exist_ok=True);(destination/'inventory.json').write_text(json.dumps(records,indent=2))
 copy(ROOT/'Cargo.lock',destination/'Cargo.lock')
 standard=pathlib.Path(subprocess.check_output(['rustc','--print','sysroot'],text=True).strip())/'share/doc/rust'
 if (standard/'COPYRIGHT-library.html').is_file():
  copy(standard/'COPYRIGHT-library.html',destination/'standard-library/COPYRIGHT-library.html')
  shutil.copytree(standard/'licenses',destination/'standard-library/licenses',dirs_exist_ok=True)
def resources(destination):
 shutil.copytree(ROOT/'packaging/resources/soundfonts',destination/'soundfonts',dirs_exist_ok=True)
 shutil.copytree(ROOT/'examples',destination/'examples',dirs_exist_ok=True)
 copy(ROOT/'LICENSE',destination/'licenses/Gosh-MIDI-GPL-3.0.txt')
 copy(ROOT/'THIRD_PARTY_NOTICES.md',destination/'licenses/THIRD_PARTY_NOTICES.md')
 rust_notices(destination/'licenses/rust')
def dependencies(binary):
 return [line.strip().split(' (',1)[0] for line in subprocess.check_output(['otool','-L',str(binary)],text=True).splitlines()[1:]]
def mac_license(library,destination):
 # Homebrew bottles often omit upstream license texts. Fetch the audited
 # formula's source archive, verify its checksum, and copy only notice files.
 parts=library.parts
 if 'Cellar' not in parts:raise RuntimeError('Unrecognized bundled library provenance: '+str(library))
 index=parts.index('Cellar');formula=parts[index+1];installed=parts[index+2]
 notice=destination/formula
 if notice.exists():return
 info=json.loads(subprocess.check_output(['brew','info','--json=v2',formula],text=True))['formulae'][0]
 version=info['versions']['stable']+(('_'+str(info['revision'])) if info.get('revision') else '')
 if version!=installed:raise RuntimeError('Formula source does not match installed library: '+formula)
 source=info['urls']['stable'];url=source['url'];checksum=source['checksum']
 if not url.startswith('https://') or not checksum:raise RuntimeError('Unpinned license source: '+formula)
 with urllib.request.urlopen(url,timeout=90,context=ssl.create_default_context()) as response:data=response.read(128*1024*1024)
 if hashlib.sha256(data).hexdigest()!=checksum:raise RuntimeError('License source checksum mismatch: '+formula)
 notice.mkdir(parents=True);(notice/'source.json').write_text(json.dumps({'formula':formula,'version':installed,'license':info['license'],'source':url,'sha256':checksum},indent=2))
 count=0
 def is_notice(name,size):
  path=pathlib.PurePosixPath(name)
  return size<1024*1024 and len(path.parts)<=4 and path.name.lower().startswith(('license','licence','copying','copyright','notice','authors'))
 if zipfile.is_zipfile(io.BytesIO(data)):
  with zipfile.ZipFile(io.BytesIO(data)) as archive:
   for member in archive.infolist():
    if not member.is_dir() and is_notice(member.filename,member.file_size):
     (notice/(str(count)+'-'+pathlib.PurePosixPath(member.filename).name)).write_bytes(archive.read(member));count+=1
 else:
  with tarfile.open(fileobj=io.BytesIO(data),mode='r:*') as archive:
   for member in archive.getmembers():
    if member.isfile() and is_notice(member.name,member.size):
     (notice/(str(count)+'-'+pathlib.PurePosixPath(member.name).name)).write_bytes(archive.extractfile(member).read());count+=1
 if not count:raise RuntimeError('Source archive has no identifiable license texts: '+formula)
 recipe=pathlib.Path(*parts[:index+3])/'.brew'/(formula+'.rb')
 if recipe.is_file():copy(recipe,notice/'formula.rb')
def mac_dependency(library,dependency):
 if dependency.startswith(('/System/','/usr/lib/')):return None
 if dependency.startswith('@loader_path/'):
  result=library.parent/dependency.removeprefix('@loader_path/')
 elif dependency.startswith('@rpath/'):
  paths=[];command=False
  for line in subprocess.check_output(['otool','-l',str(library)],text=True).splitlines():
   text=line.strip()
   if text=='cmd LC_RPATH':command=True
   elif command and text.startswith('path '):
    paths.append(text[5:].split(' (offset ',1)[0]);command=False
  candidates=[library.parent]
  for path in paths:
   if path.startswith('@loader_path'):path=str(library.parent)+path.removeprefix('@loader_path')
   if not path.startswith('@'):candidates.append(pathlib.Path(path))
  result=next((path/dependency.removeprefix('@rpath/') for path in candidates if (path/dependency.removeprefix('@rpath/')).is_file()),None)
  if result is None:raise RuntimeError('Unresolved runtime dependency: '+dependency+' in '+str(library))
 else:result=pathlib.Path(dependency)
 if not result.is_file():raise RuntimeError('Missing runtime library: '+str(result))
 return result.resolve()
def mac_libraries(initial,destination,licenses):
 destination.mkdir(parents=True,exist_ok=True);pending=[initial];seen=set()
 while pending:
  library=pathlib.Path(pending.pop()).resolve()
  if library.name in seen:continue
  seen.add(library.name);mac_license(library,licenses);target=destination/library.name;copy(library,target);target.chmod(0o755)
  for dependency in dependencies(library):
   dep=mac_dependency(library,dependency)
   if dep is not None:
    pending.append(dep);run('install_name_tool','-change',dependency,'@loader_path/'+dep.name,str(target))
  run('install_name_tool','-id','@loader_path/'+library.name,str(target))
 for library in destination.glob('*.dylib'):run('codesign','--force','--sign','-',str(library))
def windows_crt(destination):
 root=os.environ.get('VCToolsRedistDir')
 if not root:
  vswhere=pathlib.Path(os.environ.get('ProgramFiles(x86)','C:/Program Files (x86)'))/'Microsoft Visual Studio/Installer/vswhere.exe'
  installation=subprocess.check_output([str(vswhere),'-latest','-products','*','-requires','Microsoft.VisualStudio.Component.VC.Tools.x86.x64','-property','installationPath'],text=True).strip()
  versions=sorted((pathlib.Path(installation)/'VC/Redist/MSVC').glob('*'),key=lambda path:tuple(int(value) for value in path.name.split('.') if value.isdigit()))
  if versions:root=str(versions[-1])
 if not root:raise RuntimeError('Visual C++ redistributable files are unavailable. Use Developer PowerShell.')
 files=list(pathlib.Path(root).glob('x64/Microsoft.VC*.CRT/*.dll'))
 if not files:raise RuntimeError('No x64 app-local Visual C++ runtime DLLs found: '+root)
 for library in files:copy(library,destination/library.name)
 notice=destination/'resources/licenses/Microsoft-VC-runtime';notice.mkdir(parents=True,exist_ok=True)
 (notice/'README.txt').write_text('Microsoft Visual C++ app-local redistributable runtime. Redistribution is governed by the installed Visual Studio license: https://visualstudio.microsoft.com/license-terms/\n')
 (notice/'files.json').write_text(json.dumps({library.name:hashlib.sha256(library.read_bytes()).hexdigest() for library in files},indent=2))
 for license in pathlib.Path(root).glob('*.txt'):copy(license,notice/license.name)
def checksums(dist):
 files=sorted(path for path in dist.iterdir() if path.is_file() and path.name!='SHA256SUMS')
 (dist/'SHA256SUMS').write_text(''.join(hashlib.sha256(path.read_bytes()).hexdigest()+'  '+path.name+'\n' for path in files))
def main():
 parser=argparse.ArgumentParser();parser.add_argument('--target');parser.add_argument('--runtime');parser.add_argument('--licenses-only');args=parser.parse_args()
 if args.licenses_only:rust_notices(pathlib.Path(args.licenses_only));return
 version=tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version'];host=platform.system();arch=platform.machine().lower();dist=ROOT/'dist';dist.mkdir(exist_ok=True)
 build=ROOT/'target';build=build/args.target if args.target else build;binary=build/'release'/('dmidiplayer.exe' if host=='Windows' else 'dmidiplayer')
 if not binary.is_file():raise RuntimeError('Build the release binary before packaging: '+str(binary))
 stem=f'gosh-midi-{version}-{args.target or (host.lower()+"-"+arch)}';stage=dist/stem
 if stage.exists():shutil.rmtree(stage)
 if host=='Darwin':
  app=stage/'Gosh MIDI Player.app';mac=app/'Contents/MacOS';res=app/'Contents/Resources';copy(binary,mac/'dmidiplayer');resources(res)
  info=plistlib.loads((ROOT/'packaging/macos/Info.plist').read_bytes());info['CFBundleVersion']=version.split('-')[0];info['CFBundleShortVersionString']=version.split('-')[0];copy(ROOT/'packaging/macos/Info.plist',app/'Contents/Info.plist');(app/'Contents/Info.plist').write_bytes(plistlib.dumps(info))
  iconset=stage/'GoshMIDI.iconset';iconset.mkdir();icon=ROOT/'icons/dmidiplayer_128x128.png'
  for size in [16,32,128,256,512]:
   for scale in [1,2]:run('sips','-z',str(size*scale),str(size*scale),str(icon),'--out',str(iconset/f'icon_{size}x{size}{"@2x" if scale==2 else ""}.png'))
  run('iconutil','-c','icns',str(iconset),'-o',str(res/'GoshMIDI.icns'));shutil.rmtree(iconset)
  prefix=pathlib.Path(subprocess.check_output(['brew','--prefix','fluidsynth'],text=True).strip());library=next((prefix/'lib').glob('libfluidsynth.*.dylib'));mac_libraries(library,res/'lib',res/'licenses');(res/'lib/libfluidsynth.3.dylib').symlink_to(library.resolve().name)
  license_dir=res/'licenses';copy(prefix/'share/doc/fluidsynth/LICENSE',license_dir/'FluidSynth.txt') if (prefix/'share/doc/fluidsynth/LICENSE').exists() else copy(ROOT/'packaging/FluidSynth-LICENSE',license_dir/'FluidSynth.txt')
  permissions(app);run('codesign','--force','--deep','--sign','-',str(app));run('codesign','--verify','--deep','--strict',str(app));run('ditto','-c','-k','--sequesterRsrc','--keepParent',str(app),str(dist/(stem+'.zip')))
 elif host=='Windows':
  stage.mkdir();copy(binary,stage/'dmidiplayer.exe');resources(stage/'resources')
  runtime=pathlib.Path(args.runtime or ROOT/'packaging/windows/vcpkg_installed/x64-windows')
  dlls=list((runtime/'bin').glob('*.dll'))
  if not dlls:raise RuntimeError('No FluidSynth runtime DLLs found: '+str(runtime))
  for dll in dlls:copy(dll,stage/dll.name)
  windows_crt(stage)
  for license in (runtime/'share').glob('*/copyright'):copy(license,stage/'resources/licenses'/license.parent.name/'copyright')
  copy(ROOT/'icons/dmidiplayer_128x128.png',stage/'resources/icon.png')
  png=(ROOT/'icons/dmidiplayer_128x128.png').read_bytes();icon=stage/'resources/GoshMIDI.ico';icon.write_bytes(struct.pack('<HHHBBBBHHII',0,1,1,128,128,0,0,1,32,len(png),22)+png)
  copy(ROOT/'packaging/windows/WebView2Setup.exe',stage/'WebView2Setup.exe')
  files=[]
  for path in sorted(stage.rglob('*')):
   if not path.is_file():continue
   relative=path.relative_to(stage);files.append((path,relative))
  # WiX 4 generates a per-user MSI, with no administrator requirement.
  import xml.etree.ElementTree as ET
  namespace='http://wixtoolset.org/schemas/v4/wxs';ET.register_namespace('',namespace)
  def element(name,attrs=None):return ET.Element('{'+namespace+'}'+name,attrs or {})
  wix=element('Wix');package=element('Package',{'Name':'Gosh MIDI Player','Manufacturer':'Gosh MIDI Player contributors','Version':version.split('-')[0],'UpgradeCode':'60BDF0E2-8D79-48C9-863B-1DBA53F550CF','Scope':'perUser'});wix.append(package)
  package.append(element('Icon',{'Id':'GoshMIDIIcon','SourceFile':str(icon)}));package.append(element('Property',{'Id':'ARPPRODUCTICON','Value':'GoshMIDIIcon'}))
  package.append(element('MajorUpgrade',{'DowngradeErrorMessage':'A newer Gosh MIDI Player version is installed.'}));package.append(element('MediaTemplate',{'EmbedCab':'yes'}));directory=element('StandardDirectory',{'Id':'LocalAppDataFolder'});package.append(directory);install=element('Directory',{'Id':'INSTALLFOLDER','Name':'Gosh MIDI Player'});directory.append(install)
  directories={pathlib.Path('.'):install};feature=element('Feature',{'Id':'Main','Title':'Gosh MIDI Player','Level':'1'});package.append(feature)
  for index,(path,relative) in enumerate(files):
   current=pathlib.Path('.')
   for part in relative.parts[:-1]:
    child=current/part
    if child not in directories:
     node=element('Directory',{'Id':'D'+str(len(directories)),'Name':part});directories[current].append(node);directories[child]=node
    current=child
   component=element('Component',{'Id':'C'+str(index),'Guid':'*'});directories[current].append(component);component.append(element('File',{'Id':'F'+str(index),'Source':str(path)}));feature.append(element('ComponentRef',{'Id':'C'+str(index)}));component.append(element('RegistryValue',{'Root':'HKCU','Key':'Software\\Gosh MIDI Player\\Components','Name':hashlib.sha256(str(relative).encode()).hexdigest(),'Type':'integer','Value':'1','KeyPath':'yes'}))
   if relative==pathlib.Path('WebView2Setup.exe'):
    runtime_action=element('CustomAction',{'Id':'InstallWebView2','FileRef':'F'+str(index),'ExeCommand':'/silent /install','Execute':'deferred','Impersonate':'yes','Return':'check'});package.append(runtime_action)
    sequence=element('InstallExecuteSequence');package.append(sequence);action=element('Custom',{'Action':'InstallWebView2','After':'InstallFiles','Condition':'NOT Installed'});sequence.append(action)
   if relative==pathlib.Path('dmidiplayer.exe'):
    component.append(element('Shortcut',{'Id':'DesktopShortcut','Directory':'ProgramMenuFolder','Name':'Gosh MIDI Player','WorkingDirectory':'INSTALLFOLDER','Target':'[INSTALLFOLDER]dmidiplayer.exe','Icon':'GoshMIDIIcon'}))
  package.append(element('StandardDirectory',{'Id':'ProgramMenuFolder'}));source=dist/'installer.wxs';ET.ElementTree(wix).write(source,encoding='utf-8',xml_declaration=True)
  run('wix','build','-arch','x64','-o',str(dist/(stem+'.msi')),str(source));source.unlink()
  with zipfile.ZipFile(dist/(stem+'.zip'),'w',zipfile.ZIP_DEFLATED) as archive:
   for path,relative in files:archive.write(path,relative)
 else:
  copy(binary,stage/'bin/dmidiplayer');resources(stage/'bin/resources');copy(ROOT/'com.goshapps.GoshMIDI.desktop',stage/'share/applications/com.goshapps.GoshMIDI.desktop');copy(ROOT/'icons/dmidiplayer_128x128.png',stage/'share/icons/hicolor/128x128/apps/com.goshapps.GoshMIDI.png')
  permissions(stage)
  with tarfile.open(dist/(stem+'.tar.gz'),'w:gz') as archive:archive.add(stage,arcname=stem)
 if host=='Linux':
  debroot=stage/'deb';copy(binary,debroot/'usr/bin/dmidiplayer');resources(debroot/'usr/share/dmidiplayer');copy(ROOT/'com.goshapps.GoshMIDI.desktop',debroot/'usr/share/applications/com.goshapps.GoshMIDI.desktop');copy(ROOT/'com.goshapps.GoshMIDI.metainfo.xml',debroot/'usr/share/metainfo/com.goshapps.GoshMIDI.metainfo.xml');copy(ROOT/'icons/dmidiplayer_128x128.png',debroot/'usr/share/icons/hicolor/128x128/apps/com.goshapps.GoshMIDI.png')
  control=debroot/'DEBIAN';control.mkdir();(control/'control').write_text(f'Package: gosh-midi-player\nVersion: {version.replace("-","~",1)}\nArchitecture: amd64\nMaintainer: Gosh MIDI Player contributors\nDepends: libwebkit2gtk-4.1-0 (>= 2.40), libgtk-3-0t64 | libgtk-3-0, libasound2t64 | libasound2, libfluidsynth3, libxdo3, libayatana-appindicator3-1, xdg-desktop-portal, xdg-desktop-portal-gtk | xdg-desktop-portal-kde\nDescription: Cross-platform MIDI and karaoke player built with Rust and Dioxus Desktop\n')
  permissions(debroot);run('dpkg-deb','--root-owner-group','--build',str(debroot),str(dist/(stem+'.deb')))
 checksums(dist);print('Packaged '+stem)
if __name__=='__main__':main()
