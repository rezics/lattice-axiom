"""Regression tests for the source-control content boundary."""
import unittest
from repository_check import text_error


class TextBoundaryTests(unittest.TestCase):
    def test_disguised_binary_is_rejected(self):
        self.assertIsNotNone(text_error('asset.txt', b'PNG\0payload'))
        self.assertIsNotNone(text_error('asset.txt', b'\xff\xfe'))

    def test_media_extension_cannot_be_bypassed_with_text(self):
        self.assertIsNotNone(text_error('texture.PNG', b'pointer to image'))

    def test_shader_and_localized_text_are_allowed(self):
        self.assertIsNone(text_error('water.wgsl', b'@fragment fn fragment() {}'))
        self.assertIsNone(text_error('zh.json', '{"label":"设置"}'.encode()))


if __name__ == '__main__':
    unittest.main()
