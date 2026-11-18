from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="ImportRequest")


@_attrs_define
class ImportRequest:
    """An import of a completed upload (ADR-0084 d15, I4).

    Attributes:
        upload_id (str):
        target (str): An in-progress workspace by id or name, or a bare name the import creates in progress.
        dry_run (bool | Unset):  Default: False.
        allow_older (bool | Unset):  Default: False.
        expect_key (None | str | Unset): The source key's fingerprint; required on the first import from a source (R11).
        expect_tenant (None | str | Unset): The source tenant id the bundle must name (R13).
    """

    upload_id: str
    target: str
    dry_run: bool | Unset = False
    allow_older: bool | Unset = False
    expect_key: None | str | Unset = UNSET
    expect_tenant: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        upload_id = self.upload_id

        target = self.target

        dry_run = self.dry_run

        allow_older = self.allow_older

        expect_key: None | str | Unset
        if isinstance(self.expect_key, Unset):
            expect_key = UNSET
        else:
            expect_key = self.expect_key

        expect_tenant: None | str | Unset
        if isinstance(self.expect_tenant, Unset):
            expect_tenant = UNSET
        else:
            expect_tenant = self.expect_tenant

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "upload_id": upload_id,
                "target": target,
            }
        )
        if dry_run is not UNSET:
            field_dict["dry_run"] = dry_run
        if allow_older is not UNSET:
            field_dict["allow_older"] = allow_older
        if expect_key is not UNSET:
            field_dict["expect_key"] = expect_key
        if expect_tenant is not UNSET:
            field_dict["expect_tenant"] = expect_tenant

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        upload_id = d.pop("upload_id")

        target = d.pop("target")

        dry_run = d.pop("dry_run", UNSET)

        allow_older = d.pop("allow_older", UNSET)

        def _parse_expect_key(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        expect_key = _parse_expect_key(d.pop("expect_key", UNSET))

        def _parse_expect_tenant(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        expect_tenant = _parse_expect_tenant(d.pop("expect_tenant", UNSET))

        import_request = cls(
            upload_id=upload_id,
            target=target,
            dry_run=dry_run,
            allow_older=allow_older,
            expect_key=expect_key,
            expect_tenant=expect_tenant,
        )

        return import_request
