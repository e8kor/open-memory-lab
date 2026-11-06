from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.anchor_hasher import AnchorHasher
from ..types import UNSET, Unset

T = TypeVar("T", bound="Anchor")


@_attrs_define
class Anchor:
    """The origin the author declares (ADR-0057 d1-d3, d21): signed in the submission event, immutable per version, shown
    to the reviewer, never fetched by the server, and clear metadata that crypto-shredding never erases. Exactly one of
    repo (with rev and path) and uri; hash and hasher together or neither. Any other shape is request.validation naming
    the field, such as /anchor/path; a scan finding in repo, uri, path or fragment is submission.scan_veto.

        Attributes:
            repo (str | Unset): A repository, scheme, credentials and .git dropped (github.com/e8kor/open-memory-lab); no
                segment is . or .. (ADR-0057 d3). As a filter, rows whose anchor names it; unanchored rows are left out (d4).
            uri (str | Unset): A page outside a repository, a Delta table (ADR-0062 d5) or a chat turn's session (ADR-0063
                d4, d5): an absolute https, s3, gs or file URL with no user information, no query and no fragment; an s3 or gs
                bucket is a DNS-safe name, and an s3, gs or file path has no trailing slash. A chat uri is
                chat://<harness>/<session>, the session 32 lower-case hex digits derived by the plugin, never the harness's own
                id; then rev is refused and fragment, hash and hasher oml-turn/1 are required.
            rev (str | Unset): With repo, required: a full git object id, 40 or 64 lower-case hex digits. With uri,
                optional: a page version or a Delta table version in decimal, printable ASCII without whitespace.
            path (str | Unset): With repo, required; with uri, refused. NFC, relative POSIX, at most 1,024 UTF-8 bytes, no
                empty, . or .. segment, no leading /, no backslash or control character.
            fragment (str | Unset): The part of the file or page: a heading slug, an ADR decision handle dN, a line span
                L<n> or L<n>-L<m> (1 <= n <= m <= 10000000), or a Delta column col=<path> (ADR-0062 d5); page=<n> is reserved
                and refused. NFC, no whitespace, control character or #. Absent, the whole file, page or table (ADR-0057 d3,
                d21). With a chat uri, required: turn=<n> or turn=<n>-<m>, 1 <= n <= m <= 100000, the person's prompts counted
                from 1; turn= is refused with any other anchor (ADR-0063 d4).
            hash_ (str | Unset): Digest of the fragment's text, or the file's, under hasher.
            hasher (AnchorHasher | Unset): oml-md/1: NFC, LF, whitespace runs to one space, trimmed, SHA-256 (ADR-0057 d7).
                oml-turn/1: HMAC-SHA-256 of the person's prompt as the harness passed it, UTF-8 after NFC, keyed by the person's
                chat salt, which never leaves their machine; with a chat uri alone (ADR-0063 d4). oml-rustdoc/1: a module doc
                block without its //! markers, then oml-md/1 (ADR-0061 d7). oml-crate/1: SHA-256 of one line per fact of a
                crate: name, description, root doc, binaries, internal dependencies and dependents (ADR-0061 d7).  oml-delta-
                table/1, oml-delta-schema/1 and oml-delta-col/1 hash the facts a Delta table claim, its schema claim and a
                column claim state (ADR-0062 d7). The server computes none of the rust or delta hashes.
    """

    repo: str | Unset = UNSET
    uri: str | Unset = UNSET
    rev: str | Unset = UNSET
    path: str | Unset = UNSET
    fragment: str | Unset = UNSET
    hash_: str | Unset = UNSET
    hasher: AnchorHasher | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        repo = self.repo

        uri = self.uri

        rev = self.rev

        path = self.path

        fragment = self.fragment

        hash_ = self.hash_

        hasher: str | Unset = UNSET
        if not isinstance(self.hasher, Unset):
            hasher = self.hasher.value

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if repo is not UNSET:
            field_dict["repo"] = repo
        if uri is not UNSET:
            field_dict["uri"] = uri
        if rev is not UNSET:
            field_dict["rev"] = rev
        if path is not UNSET:
            field_dict["path"] = path
        if fragment is not UNSET:
            field_dict["fragment"] = fragment
        if hash_ is not UNSET:
            field_dict["hash"] = hash_
        if hasher is not UNSET:
            field_dict["hasher"] = hasher

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        repo = d.pop("repo", UNSET)

        uri = d.pop("uri", UNSET)

        rev = d.pop("rev", UNSET)

        path = d.pop("path", UNSET)

        fragment = d.pop("fragment", UNSET)

        hash_ = d.pop("hash", UNSET)

        _hasher = d.pop("hasher", UNSET)
        hasher: AnchorHasher | Unset
        if isinstance(_hasher, Unset):
            hasher = UNSET
        else:
            hasher = AnchorHasher(_hasher)

        anchor = cls(
            repo=repo,
            uri=uri,
            rev=rev,
            path=path,
            fragment=fragment,
            hash_=hash_,
            hasher=hasher,
        )

        return anchor
