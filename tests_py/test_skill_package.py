from pathlib import Path
import re
import unittest


ROOT = Path(__file__).parents[1]
SKILL = ROOT / "skills" / "textcon-bundle-codebase"


class SkillPackageTests(unittest.TestCase):
    def test_skill_contains_only_required_files(self):
        files = {
            path.relative_to(SKILL).as_posix()
            for path in SKILL.rglob("*")
            if path.is_file() and "__pycache__" not in path.parts
        }
        self.assertEqual(
            files,
            {
                "SKILL.md",
                "agents/openai.yaml",
                "scripts/bundle_codebase.py",
            },
        )

    def test_frontmatter_has_only_name_and_description(self):
        contents = (SKILL / "SKILL.md").read_text(encoding="utf-8")
        match = re.match(r"\A---\n(.*?)\n---\n", contents, re.DOTALL)
        self.assertIsNotNone(match)
        keys = [line.split(":", 1)[0] for line in match.group(1).splitlines()]
        self.assertEqual(keys, ["name", "description"])
        self.assertNotIn("TODO", contents)

    def test_openai_metadata_is_current(self):
        contents = (SKILL / "agents" / "openai.yaml").read_text(encoding="utf-8")
        self.assertIn('display_name: "Bundle Codebase with textcon"', contents)
        self.assertIn(
            'short_description: "Bundle codebases into timestamped Markdown"',
            contents,
        )
        self.assertIn("$textcon-bundle-codebase", contents)
        self.assertNotIn("TODO", contents)


if __name__ == "__main__":
    unittest.main()
