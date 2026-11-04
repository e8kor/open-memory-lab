from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.client_registration_request_grant_types_item import (
    ClientRegistrationRequestGrantTypesItem,
)
from ..models.client_registration_request_response_types_item import (
    ClientRegistrationRequestResponseTypesItem,
)
from ..models.client_registration_request_token_endpoint_auth_method import (
    ClientRegistrationRequestTokenEndpointAuthMethod,
)
from ..types import UNSET, Unset

T = TypeVar("T", bound="ClientRegistrationRequest")


@_attrs_define
class ClientRegistrationRequest:
    """
    Attributes:
        redirect_uris (list[str]): Each inside the allowlist: exactly https://claude.ai/api/mcp/auth_callback, matched
            byte for byte on the JSON string value, never after URL parsing or any further decoding.
        client_name (str): Required, since the authorization page shows it; absent, it is request.validation
            (invalid_client_metadata).
        token_endpoint_auth_method (ClientRegistrationRequestTokenEndpointAuthMethod | Unset): Absent, it is registered
            as none and echoed as none; any other value is request.validation (invalid_client_metadata).
        grant_types (list[ClientRegistrationRequestGrantTypesItem] | Unset):
        response_types (list[ClientRegistrationRequestResponseTypesItem] | Unset):
    """

    redirect_uris: list[str]
    client_name: str
    token_endpoint_auth_method: (
        ClientRegistrationRequestTokenEndpointAuthMethod | Unset
    ) = UNSET
    grant_types: list[ClientRegistrationRequestGrantTypesItem] | Unset = UNSET
    response_types: list[ClientRegistrationRequestResponseTypesItem] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        redirect_uris = self.redirect_uris

        client_name = self.client_name

        token_endpoint_auth_method: str | Unset = UNSET
        if not isinstance(self.token_endpoint_auth_method, Unset):
            token_endpoint_auth_method = self.token_endpoint_auth_method.value

        grant_types: list[str] | Unset = UNSET
        if not isinstance(self.grant_types, Unset):
            grant_types = []
            for grant_types_item_data in self.grant_types:
                grant_types_item = grant_types_item_data.value
                grant_types.append(grant_types_item)

        response_types: list[str] | Unset = UNSET
        if not isinstance(self.response_types, Unset):
            response_types = []
            for response_types_item_data in self.response_types:
                response_types_item = response_types_item_data.value
                response_types.append(response_types_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "redirect_uris": redirect_uris,
                "client_name": client_name,
            }
        )
        if token_endpoint_auth_method is not UNSET:
            field_dict["token_endpoint_auth_method"] = token_endpoint_auth_method
        if grant_types is not UNSET:
            field_dict["grant_types"] = grant_types
        if response_types is not UNSET:
            field_dict["response_types"] = response_types

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        redirect_uris = cast(list[str], d.pop("redirect_uris"))

        client_name = d.pop("client_name")

        _token_endpoint_auth_method = d.pop("token_endpoint_auth_method", UNSET)
        token_endpoint_auth_method: (
            ClientRegistrationRequestTokenEndpointAuthMethod | Unset
        )
        if isinstance(_token_endpoint_auth_method, Unset):
            token_endpoint_auth_method = UNSET
        else:
            token_endpoint_auth_method = (
                ClientRegistrationRequestTokenEndpointAuthMethod(
                    _token_endpoint_auth_method
                )
            )

        _grant_types = d.pop("grant_types", UNSET)
        grant_types: list[ClientRegistrationRequestGrantTypesItem] | Unset = UNSET
        if _grant_types is not UNSET:
            grant_types = []
            for grant_types_item_data in _grant_types:
                grant_types_item = ClientRegistrationRequestGrantTypesItem(
                    grant_types_item_data
                )

                grant_types.append(grant_types_item)

        _response_types = d.pop("response_types", UNSET)
        response_types: list[ClientRegistrationRequestResponseTypesItem] | Unset = UNSET
        if _response_types is not UNSET:
            response_types = []
            for response_types_item_data in _response_types:
                response_types_item = ClientRegistrationRequestResponseTypesItem(
                    response_types_item_data
                )

                response_types.append(response_types_item)

        client_registration_request = cls(
            redirect_uris=redirect_uris,
            client_name=client_name,
            token_endpoint_auth_method=token_endpoint_auth_method,
            grant_types=grant_types,
            response_types=response_types,
        )

        client_registration_request.additional_properties = d
        return client_registration_request

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
