#!/usr/bin/env python3
"""Prepare a disposable source copy with fully intercepted API and local media.

Build this copy with xcodebuild, then copy playback-fixture.mp4 into its .app
before simulator installation. No changes are made to the production project.
"""
import pathlib
import shutil
import sys

root = pathlib.Path(__file__).resolve().parents[1]
stage = pathlib.Path(sys.argv[1]).resolve()
stage.mkdir(parents=True, exist_ok=False)
for folder in ('MelloTV', 'MelloTV.xcodeproj'):
    shutil.copytree(root / folder, stage / folder)
api = stage / 'MelloTV/Services/MelloTVAPI.swift'
source = api.read_text().replace('let configuration = URLSessionConfiguration.ephemeral',
    'let configuration = URLSessionConfiguration.ephemeral\n        configuration.protocolClasses = [SimulatorPlaybackFixture.self]', 1)
api.write_text(source + '\n' + (root / 'tools/SimulatorPlaybackFixture.swift').read_text())
address = stage / 'MelloTV/Services/ServerAddress.swift'
address.write_text(address.read_text().replace('http://127.0.0.1:8080', 'https://fixture.invalid'))
for relative in ('Features/Playback/PlayerView.swift', 'Features/Playback/PreparedPlaybackService.swift'):
    path = stage / 'MelloTV' / relative
    source = path.read_text()
    for variable in ('source', 'recovered', 'renewed', 'upgraded'):
        source = source.replace(f'AVPlayerItem(url: {variable}.url)',
            f'AVPlayerItem(url: SimulatorPlaybackFixture.playbackURL({variable}))')
    path.write_text(source)
print(stage)
