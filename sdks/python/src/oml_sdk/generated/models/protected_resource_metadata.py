from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.protected_resource_metadata_bearer_methods_supported_item import (
    ProtectedResourceMetadataBearerMethodsSupportedItem,
)
from ..models.protected_resource_metadata_scopes_supported_item import (
    ProtectedResourceMetadataScopesSupportedItem,
)
from ..types import UNSET, Unset

T = TypeVar("T", bound="ProtectedResourceMetadata")


@_attrs_define
class ProtectedResourceMetadata:
    """
    Attributes:
        resource (str): Exactly <issuer>/mcp (RFC 9728 §3.3).
        authorization_servers (list[str]):
        scopes_supported (list[ProtectedResourceMetadataScopesSupportedItem] | Unset):
        bearer_methods_supported (list[ProtectedResourceMetadataBearerMethodsSupportedItem] | Unset):
        resource_name (str | Unset):
    """

    resource: str
    authorization_servers: list[str]
    scopes_supported: list[ProtectedResourceMetadataScopesSupportedItem] | Unset = UNSET
    bearer_methods_supported: (
        list[ProtectedResourceMetadataBearerMethodsSupportedItem] | Unset
    ) = UNSET
    resource_name: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        resource = self.resource

        authorization_servers = self.authorization_servers

        scopes_supported: list[str] | Unset = UNSET
        if not isinstance(self.scopes_supported, Unset):
            scopes_supported = []
            for scopes_supported_item_data in self.scopes_supported:
                scopes_supported_item = scopes_supported_item_data.value
                scopes_supported.append(scopes_supported_item)

        bearer_methods_supported: list[str] | Unset = UNSET
        if not isinstance(self.bearer_methods_supported, Unset):
            bearer_methods_supported = []
            for bearer_methods_supported_item_data in self.bearer_methods_supported:
                bearer_methods_supported_item = bearer_methods_supported_item_data.value
                bearer_methods_supported.append(bearer_methods_supported_item)

        resource_name = self.resource_name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "resource": resource,
                "authorization_servers": authorization_servers,
            }
        )
        if scopes_supported is not UNSET:
            field_dict["scopes_supported"] = scopes_supported
        if bearer_methods_supported is not UNSET:
            field_dict["bearer_methods_supported"] = bearer_methods_supported
        if resource_name is not UNSET:
            field_dict["resource_name"] = resource_name

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        resource = d.pop("resource")

        authorization_servers = cast(list[str], d.pop("authorization_servers"))

        _scopes_supported = d.pop("scopes_supported", UNSET)
        scopes_supported: list[ProtectedResourceMetadataScopesSupportedItem] | Unset = (
            UNSET
        )
        if _scopes_supported is not UNSET:
            scopes_supported = []
            for scopes_supported_item_data in _scopes_supported:
                scopes_supported_item = ProtectedResourceMetadataScopesSupportedItem(
                    scopes_supported_item_data
                )

                scopes_supported.append(scopes_supported_item)

        _bearer_methods_supported = d.pop("bearer_methods_supported", UNSET)
        bearer_methods_supported: (
            list[ProtectedResourceMetadataBearerMethodsSupportedItem] | Unset
        ) = UNSET
        if _bearer_methods_supported is not UNSET:
            bearer_methods_supported = []
            for bearer_methods_supported_item_data in _bearer_methods_supported:
                bearer_methods_supported_item = (
                    ProtectedResourceMetadataBearerMethodsSupportedItem(
                        bearer_methods_supported_item_data
                    )
                )

                bearer_methods_supported.append(bearer_methods_supported_item)

        resource_name = d.pop("resource_name", UNSET)

        protected_resource_metadata = cls(
            resource=resource,
            authorization_servers=authorization_servers,
            scopes_supported=scopes_supported,
            bearer_methods_supported=bearer_methods_supported,
            resource_name=resource_name,
        )

        protected_resource_metadata.additional_properties = d
        return protected_resource_metadata

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
