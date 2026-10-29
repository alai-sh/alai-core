# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""A collection's metadata, as ansible-galaxy and ansible-core read it."""

from ._native import CollectionEdaRuntime as EdaRuntime
from ._native import CollectionManifest as Manifest
from ._native import CollectionMetadata as Metadata
from ._native import CollectionMetadataError as MetadataError
from ._native import CollectionRemoval as Removal
from ._native import CollectionRoutingEntry as RoutingEntry
from ._native import CollectionRuntime as Runtime
from ._native import collection_check_name as check_name

__all__ = ["EdaRuntime", "Manifest", "Metadata", "MetadataError", "Removal", "RoutingEntry", "Runtime", "check_name"]
