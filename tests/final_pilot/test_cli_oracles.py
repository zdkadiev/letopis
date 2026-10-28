"""TK-30 test cli oracles; model-free. Shared fixtures are private."""
from unittest import mock
from oracles import cli
from _oracle_fixtures import (
    TempDirTestCase,
)
import unittest


INVALID = ('{"ok":false,"error":{"code":"invalid_request","message":"invalid command input",'
           '"details":{}}}')


GENERAL_HELP = """Usage: letopis COMMAND < request.json
Every command reads one JSON object on standard input.
Examples:
  echo '{"paths":["a.md"]}' | letopis check
  echo '{"element_ids":["a.md"]}' | letopis inspect
  echo '{"query":"q","limit":5}' | letopis search
  echo '{"element_ids":["a.md"],"mode":"relations","direction":"outgoing","depth":1,"limit":5}' | letopis traverse
  echo '{}' | letopis reindex
  echo '{}' | letopis status
  letopis embed --drain
"""


COMMAND_HELP = {
    "check": "paths (optional): repository paths\n",
    "inspect": "element_ids (required): ids\n",
    "search": "query (required)\nlimit (required)\nfilters (optional)\n",
    "traverse": ("element_ids (required)\nmode (required)\ndirection (required)\n"
                 "depth (required)\nlimit (required)\nedge_types (optional)\n"
                 "edge_limit (optional)\n"),
    "reindex": "input: {}\n",
    "status": "input: {}\n",
}


class VersionOracleTests(TempDirTestCase):
    def test_exact_version_without_state_passes(self):
        # When
        result = self.observe('[ "$*" = "--version" ] && printf "letopis 0.0.0\\n" && exit 0')

        # Then
        self.assertEqual(result, {"version_value": True, "version_no_stdin_or_state": True})

    def test_wrong_version_or_json_failure_fails(self):
        for body in ('[ "$*" = "--version" ] && printf "letopis 0.0.1\\n" && exit 0', ":"):
            with self.subTest(body):
                # When
                result = self.observe(body)

                # Then
                self.assertFalse(result["version_value"])

    def test_reading_stdin_or_writing_state_fails(self):
        # Given
        reader = 'cat >/dev/null; printf "letopis 0.0.0\\n"; exit 0'
        writer = 'touch "$HOME/.state" .letopis; printf "letopis 0.0.0\\n"; exit 0'

        # When
        with mock.patch.object(cli, "FLAG_WATCHDOG_SECONDS", 1):
            hung = self.observe(reader)
        stateful = self.observe(writer)

        # Then
        self.assertEqual(hung, {"version_value": False, "version_no_stdin_or_state": False})
        self.assertEqual(stateful, {"version_value": True, "version_no_stdin_or_state": False})

    def observe(self, body):
        checks = cli.observe_version(self.write_binary(body), self.evidence)
        return {check["id"]: check["passed"] for check in checks}

    def write_binary(self, body):
        binary = self.tmp / "letopis"
        binary.write_text(f"#!/bin/sh\n{body}\nprintf '%s\\n' '{INVALID}'\nexit 1\n")
        binary.chmod(0o755)
        return binary


class HelpOracleTests(TempDirTestCase):
    def test_general_help_must_list_the_existing_detached_drain_command(self):
        # Given
        missing = GENERAL_HELP.replace("  letopis embed --drain\n", "")

        # When / Then
        self.assertTrue(cli.find_help_gaps(missing))
        self.assertEqual(cli.find_help_gaps(GENERAL_HELP), [])

    def test_general_help_needs_json_stdin_and_every_example(self):
        # Given
        without_traverse = GENERAL_HELP.replace("| letopis traverse", "")

        # When
        complete = cli.observe_help(self.write_binary(GENERAL_HELP), self.evidence)
        missing = cli.observe_help(self.write_binary(without_traverse), self.evidence)

        # Then
        self.assertEqual([c["passed"] for c in complete], [True, True])
        self.assertEqual([c["passed"] for c in missing], [False, True])
        self.assertIn("traverse", missing[0]["evidence"])

    def test_command_help_explains_forms_and_keeps_invalid_argv(self):
        # When
        good = cli.observe_command_help(self.write_binary(GENERAL_HELP), self.evidence)

        # Then
        self.assertEqual([c["passed"] for c in good], [True, True])

    def test_help_for_unknown_command_or_wrong_presence_fails(self):
        # Given
        lenient = 'if [ "$2" = "--help" ] && [ "$1" = "bogus" ]; then echo help; exit 0; fi'
        wrong = dict(COMMAND_HELP, search="query (required)\nlimit (optional)\nfilters (optional)\n")

        # When
        accepted = cli.observe_command_help(self.write_binary(GENERAL_HELP, lenient),
                                            self.evidence)
        with mock.patch.dict(COMMAND_HELP, wrong):
            misdescribed = cli.observe_command_help(self.write_binary(GENERAL_HELP),
                                                    self.evidence)

        # Then
        self.assertEqual([c["passed"] for c in accepted], [True, False])
        self.assertEqual([c["passed"] for c in misdescribed], [False, True])
        self.assertIn("search --help does not mark limit as required", misdescribed[0]["evidence"])

    def write_binary(self, help_text, extra=""):
        binary = self.tmp / "letopis"
        lines = ["#!/bin/sh", extra,
                 'if [ "$*" = "--version" ]; then printf "letopis 0.0.0\\n"; exit 0; fi',
                 f"if [ \"$*\" = \"--help\" ]; then cat <<'EOF'\n{help_text}EOF\nexit 0; fi"]
        for command, text in COMMAND_HELP.items():
            lines.append(f"if [ \"$*\" = \"{command} --help\" ]; then cat <<'EOF'\n"
                         f"{text}EOF\nexit 0; fi")
        lines += ['if [ "$*" = "embed --drain --help" ]; then echo "internal drain"; exit 0; fi',
                  f"printf '%s\\n' '{INVALID}'", "exit 1"]
        binary.write_text("\n".join(lines) + "\n")
        binary.chmod(0o755)
        return binary


if __name__ == "__main__":
    unittest.main()
