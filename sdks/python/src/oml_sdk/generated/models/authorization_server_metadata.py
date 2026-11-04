from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.authorization_server_metadata_code_challenge_methods_supported_item import (
    AuthorizationServerMetadataCodeChallengeMethodsSupportedItem,
)
from ..models.authorization_server_metadata_grant_types_supported_item import (
    AuthorizationServerMetadataGrantTypesSupportedItem,
)
from ..models.authorization_server_metadata_response_types_supported_item import (
    AuthorizationServerMetadataResponseTypesSupportedItem,
)
from ..models.authorization_server_metadata_scopes_supported_item import (
    AuthorizationServerMetadataScopesSupportedItem,
)
from ..models.authorization_server_metadata_token_endpoint_auth_methods_supported_item import (
    AuthorizationServerMetadataTokenEndpointAuthMethodsSupportedItem,
)
from ..types import UNSET, Unset

T = TypeVar("T", bound="AuthorizationServerMetadata")


@_attrs_define
class AuthorizationServerMetadata:
    """
    Attributes:
        issuer (str):
        authorization_endpoint (str):
        token_endpoint (str):
        response_types_supported (list[AuthorizationServerMetadataResponseTypesSupportedItem]):
        code_challenge_methods_supported (list[AuthorizationServerMetadataCodeChallengeMethodsSupportedItem]):
        registration_endpoint (str | Unset):
        grant_types_supported (list[AuthorizationServerMetadataGrantTypesSupportedItem] | Unset):
        token_endpoint_auth_methods_supported (list[AuthorizationServerMetadataTokenEndpointAuthMethodsSupportedItem] |
            Unset):
        scopes_supported (list[AuthorizationServerMetadataScopesSupportedItem] | Unset):
        client_id_metadata_document_supported (bool | Unset):
        authorization_response_iss_parameter_supported (bool | Unset): RFC 9207 §3: every authorization response,
            success or error, carries iss.
    """

    issuer: str
    authorization_endpoint: str
    token_endpoint: str
    response_types_supported: list[
        AuthorizationServerMetadataResponseTypesSupportedItem
    ]
    code_challenge_methods_supported: list[
        AuthorizationServerMetadataCodeChallengeMethodsSupportedItem
    ]
    registration_endpoint: str | Unset = UNSET
    grant_types_supported: (
        list[AuthorizationServerMetadataGrantTypesSupportedItem] | Unset
    ) = UNSET
    token_endpoint_auth_methods_supported: (
        list[AuthorizationServerMetadataTokenEndpointAuthMethodsSupportedItem] | Unset
    ) = UNSET
    scopes_supported: list[AuthorizationServerMetadataScopesSupportedItem] | Unset = (
        UNSET
    )
    client_id_metadata_document_supported: bool | Unset = UNSET
    authorization_response_iss_parameter_supported: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        issuer = self.issuer

        authorization_endpoint = self.authorization_endpoint

        token_endpoint = self.token_endpoint

        response_types_supported = []
        for response_types_supported_item_data in self.response_types_supported:
            response_types_supported_item = response_types_supported_item_data.value
            response_types_supported.append(response_types_supported_item)

        code_challenge_methods_supported = []
        for (
            code_challenge_methods_supported_item_data
        ) in self.code_challenge_methods_supported:
            code_challenge_methods_supported_item = (
                code_challenge_methods_supported_item_data.value
            )
            code_challenge_methods_supported.append(
                code_challenge_methods_supported_item
            )

        registration_endpoint = self.registration_endpoint

        grant_types_supported: list[str] | Unset = UNSET
        if not isinstance(self.grant_types_supported, Unset):
            grant_types_supported = []
            for grant_types_supported_item_data in self.grant_types_supported:
                grant_types_supported_item = grant_types_supported_item_data.value
                grant_types_supported.append(grant_types_supported_item)

        token_endpoint_auth_methods_supported: list[str] | Unset = UNSET
        if not isinstance(self.token_endpoint_auth_methods_supported, Unset):
            token_endpoint_auth_methods_supported = []
            for (
                token_endpoint_auth_methods_supported_item_data
            ) in self.token_endpoint_auth_methods_supported:
                token_endpoint_auth_methods_supported_item = (
                    token_endpoint_auth_methods_supported_item_data.value
                )
                token_endpoint_auth_methods_supported.append(
                    token_endpoint_auth_methods_supported_item
                )

        scopes_supported: list[str] | Unset = UNSET
        if not isinstance(self.scopes_supported, Unset):
            scopes_supported = []
            for scopes_supported_item_data in self.scopes_supported:
                scopes_supported_item = scopes_supported_item_data.value
                scopes_supported.append(scopes_supported_item)

        client_id_metadata_document_supported = (
            self.client_id_metadata_document_supported
        )

        authorization_response_iss_parameter_supported = (
            self.authorization_response_iss_parameter_supported
        )

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "issuer": issuer,
                "authorization_endpoint": authorization_endpoint,
                "token_endpoint": token_endpoint,
                "response_types_supported": response_types_supported,
                "code_challenge_methods_supported": code_challenge_methods_supported,
            }
        )
        if registration_endpoint is not UNSET:
            field_dict["registration_endpoint"] = registration_endpoint
        if grant_types_supported is not UNSET:
            field_dict["grant_types_supported"] = grant_types_supported
        if token_endpoint_auth_methods_supported is not UNSET:
            field_dict["token_endpoint_auth_methods_supported"] = (
                token_endpoint_auth_methods_supported
            )
        if scopes_supported is not UNSET:
            field_dict["scopes_supported"] = scopes_supported
        if client_id_metadata_document_supported is not UNSET:
            field_dict["client_id_metadata_document_supported"] = (
                client_id_metadata_document_supported
            )
        if authorization_response_iss_parameter_supported is not UNSET:
            field_dict["authorization_response_iss_parameter_supported"] = (
                authorization_response_iss_parameter_supported
            )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        issuer = d.pop("issuer")

        authorization_endpoint = d.pop("authorization_endpoint")

        token_endpoint = d.pop("token_endpoint")

        response_types_supported = []
        _response_types_supported = d.pop("response_types_supported")
        for response_types_supported_item_data in _response_types_supported:
            response_types_supported_item = (
                AuthorizationServerMetadataResponseTypesSupportedItem(
                    response_types_supported_item_data
                )
            )

            response_types_supported.append(response_types_supported_item)

        code_challenge_methods_supported = []
        _code_challenge_methods_supported = d.pop("code_challenge_methods_supported")
        for (
            code_challenge_methods_supported_item_data
        ) in _code_challenge_methods_supported:
            code_challenge_methods_supported_item = (
                AuthorizationServerMetadataCodeChallengeMethodsSupportedItem(
                    code_challenge_methods_supported_item_data
                )
            )

            code_challenge_methods_supported.append(
                code_challenge_methods_supported_item
            )

        registration_endpoint = d.pop("registration_endpoint", UNSET)

        _grant_types_supported = d.pop("grant_types_supported", UNSET)
        grant_types_supported: (
            list[AuthorizationServerMetadataGrantTypesSupportedItem] | Unset
        ) = UNSET
        if _grant_types_supported is not UNSET:
            grant_types_supported = []
            for grant_types_supported_item_data in _grant_types_supported:
                grant_types_supported_item = (
                    AuthorizationServerMetadataGrantTypesSupportedItem(
                        grant_types_supported_item_data
                    )
                )

                grant_types_supported.append(grant_types_supported_item)

        _token_endpoint_auth_methods_supported = d.pop(
            "token_endpoint_auth_methods_supported", UNSET
        )
        token_endpoint_auth_methods_supported: (
            list[AuthorizationServerMetadataTokenEndpointAuthMethodsSupportedItem]
            | Unset
        ) = UNSET
        if _token_endpoint_auth_methods_supported is not UNSET:
            token_endpoint_auth_methods_supported = []
            for (
                token_endpoint_auth_methods_supported_item_data
            ) in _token_endpoint_auth_methods_supported:
                token_endpoint_auth_methods_supported_item = (
                    AuthorizationServerMetadataTokenEndpointAuthMethodsSupportedItem(
                        token_endpoint_auth_methods_supported_item_data
                    )
                )

                token_endpoint_auth_methods_supported.append(
                    token_endpoint_auth_methods_supported_item
                )

        _scopes_supported = d.pop("scopes_supported", UNSET)
        scopes_supported: (
            list[AuthorizationServerMetadataScopesSupportedItem] | Unset
        ) = UNSET
        if _scopes_supported is not UNSET:
            scopes_supported = []
            for scopes_supported_item_data in _scopes_supported:
                scopes_supported_item = AuthorizationServerMetadataScopesSupportedItem(
                    scopes_supported_item_data
                )

                scopes_supported.append(scopes_supported_item)

        client_id_metadata_document_supported = d.pop(
            "client_id_metadata_document_supported", UNSET
        )

        authorization_response_iss_parameter_supported = d.pop(
            "authorization_response_iss_parameter_supported", UNSET
        )

        authorization_server_metadata = cls(
            issuer=issuer,
            authorization_endpoint=authorization_endpoint,
            token_endpoint=token_endpoint,
            response_types_supported=response_types_supported,
            code_challenge_methods_supported=code_challenge_methods_supported,
            registration_endpoint=registration_endpoint,
            grant_types_supported=grant_types_supported,
            token_endpoint_auth_methods_supported=token_endpoint_auth_methods_supported,
            scopes_supported=scopes_supported,
            client_id_metadata_document_supported=client_id_metadata_document_supported,
            authorization_response_iss_parameter_supported=authorization_response_iss_parameter_supported,
        )

        authorization_server_metadata.additional_properties = d
        return authorization_server_metadata

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
