#!/usr/bin/env python3
"""Run the opt-in real-WebView suite on the current OS's desktop session."""
import argparse,json,os,pathlib,subprocess,tempfile,time
parser=argparse.ArgumentParser();parser.add_argument('binary');args=parser.parse_args()
root=pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='gosh-midi-qa-') as directory:
 profile=pathlib.Path(directory);report=profile/'report.json';env=os.environ.copy();env['GOSH_MIDI_UI_REPORT']=str(report)
 with open(profile/'desktop.log','w') as log:
  process=subprocess.Popen([str(pathlib.Path(args.binary).resolve()),'--file',str(profile/'app.conf'),'--driver','Dummy','--connection','dummy',str(root/'examples/test.mid')],env=env,stdout=log,stderr=log)
  try:
   deadline=time.monotonic()+110
   while time.monotonic()<deadline:
    if process.poll() is not None:raise RuntimeError('Desktop exited: '+(profile/'desktop.log').read_text())
    if report.exists():
     result=json.loads(report.read_text())
     if result.get('complete'):
      print(json.dumps(result,indent=2));assert result.get('status')=='passed',result;break
    time.sleep(.25)
   else:raise RuntimeError('Desktop test deadline: '+(profile/'desktop.log').read_text())
  finally:
   process.terminate()
   try:process.wait(timeout=5)
   except subprocess.TimeoutExpired:process.kill();process.wait()
