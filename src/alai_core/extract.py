# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""Static extraction of a collection's plugins and their docs."""

from ._native import ExtractedCollection as Collection
from ._native import ExtractedDocFragment as DocFragment
from ._native import ExtractedDocsError as DocsError
from ._native import ExtractedFragmentLookup as FragmentLookup
from ._native import ExtractedIssue as Issue
from ._native import ExtractedPlugin as Plugin
from ._native import ExtractedPluginDocs as PluginDocs
from ._native import ExtractedRouting as Routing
from ._native import extract_collection as extract
from ._native import extract_fragment_lookup as fragment_lookup

__all__ = [
    "Collection",
    "DocFragment",
    "DocsError",
    "FragmentLookup",
    "Issue",
    "Plugin",
    "PluginDocs",
    "Routing",
    "extract",
    "fragment_lookup",
]
