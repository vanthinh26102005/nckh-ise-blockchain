"""Check real Cargo metadata invalidation without building any SP1 guest/dependency."""
import json
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import unittest

BUILD = Path(__file__).resolve().parents[1] / 'crates/sp1-e2e/script/build.rs'


class BuildProvenance(unittest.TestCase):
    def test_revision_refreshes_after_commit_source_edit_and_staging(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'repo'
            script = root / 'crates/sp1-e2e/script'
            (script / 'src').mkdir(parents=True)
            (script / 'Cargo.toml').write_text('[package]\nname="provenance-smoke"\nversion="0.0.0"\nedition="2021"\n[workspace]\n')
            (script / 'src/main.rs').write_text('fn main() { println!("{}", env!("EUDR_SOURCE_REVISION")); }\n')
            # Exercise the production build script, replacing only the expensive guest builder.
            (script / 'build.rs').write_text('mod sp1_build { pub fn build_program(_: &str) {} }\ninclude!(' + json.dumps(str(BUILD)) + ');\n')
            for package in ('policy', 'program', 'epoch-program'):
                path = script.parent / package
                (path / 'src').mkdir(parents=True)
                (path / 'src/lib.rs').write_text('// fixture source\n')
                (path / 'Cargo.toml').write_text('# fixture manifest\n')
            for package in ('e1-bench', 'e2-bench', 'epcis-mapper'):
                path = root / 'crates' / package
                (path / 'src').mkdir(parents=True)
                (path / 'src/lib.rs').write_text('// fixture source\n')
                (path / 'Cargo.toml').write_text('# fixture manifest\n')
            (root / 'crates/epcis-mapper/fixtures').mkdir()
            for path in (root / 'Cargo.toml', root / 'Cargo.lock', script.parent / 'Cargo.toml', script.parent / 'Cargo.lock'):
                path.write_text('# fixture workspace\n')
            (script / 'Cargo.lock').write_text('version = 4\n[[package]]\nname = "provenance-smoke"\nversion = "0.0.0"\n')
            env = dict(os.environ, CARGO_BUILD_JOBS='1', RAYON_NUM_THREADS='2', CARGO_TARGET_DIR=str(Path(directory) / 'target'))
            env.pop('SOURCE_REVISION', None)

            def git(*args):
                return subprocess.check_output(['git', *args], cwd=root, env=env, text=True, stderr=subprocess.DEVNULL).strip()

            def build():
                subprocess.run(['cargo', 'build', '--offline', '--quiet'], cwd=script, env=env, check=True, capture_output=True, text=True)
                return subprocess.check_output([str(Path(env['CARGO_TARGET_DIR']) / 'debug/provenance-smoke')], text=True).strip()

            git('init')
            git('config', 'user.name', 'Provenance Test')
            git('config', 'user.email', 'test@example.invalid')
            subprocess.run(['cargo', 'generate-lockfile', '--offline', '--quiet'], cwd=script, env=env, check=True)
            git('add', '.')
            git('commit', '-m', 'fixture')
            self.assertEqual(build(), git('rev-parse', 'HEAD'))
            git('commit', '--allow-empty', '-m', 'revision only')
            self.assertEqual(build(), git('rev-parse', 'HEAD'), 'commit-only changes must invalidate cached revision')
            policy = script.parent / 'policy/src/lib.rs'
            policy.write_text('// changed dependency source\n')
            self.assertEqual(build(), 'unknown-dirty-source', 'dependency edits must invalidate clean revision')
            git('add', str(policy))
            self.assertEqual(build(), 'unknown-dirty-source')
            git('commit', '-m', 'dependency changed')
            self.assertEqual(build(), git('rev-parse', 'HEAD'))
            untracked = root / 'crates/epcis-mapper/src/new.rs'
            untracked.write_text('// untracked source must not claim the committed revision\n')
            self.assertEqual(build(), 'unknown-dirty-source')
            untracked.unlink()
            archived_revision = git('rev-parse', 'HEAD')
            shutil.rmtree(root / '.git')  # Only the temporary fixture repository.
            env['SOURCE_REVISION'] = archived_revision
            self.assertEqual(build(), archived_revision, 'private Git archives must use their pinned build revision')
            env['SOURCE_REVISION'] = 'placeholder'
            self.assertEqual(build(), 'unknown', 'placeholder revisions cannot become proof provenance')


if __name__ == '__main__':
    unittest.main()
