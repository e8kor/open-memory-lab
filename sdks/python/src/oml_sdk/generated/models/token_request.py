from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.token_request_grant_type import TokenRequestGrantType
from ..models.token_request_scope import TokenRequestScope
from ..types import UNSET, Unset

T = TypeVar("T", bound="TokenRequest")


@_attrs_define
class TokenRequest:
    """
    Attributes:
        grant_type (TokenRequestGrantType):
        client_id (str):
        code (str | Unset): authorization_code only.
        redirect_uri (str | Unset): authorization_code only. Required exactly when the authorize request carried it, and
            then equal to it; when that request omitted it, omitted or equal to the client's one registered URI; otherwise
            oauth.authorization_code_invalid.
        code_verifier (str | Unset): authorization_code only; S256 against the authorization request's code_challenge.
        refresh_token (str | Unset): refresh_token only.
        resource (str | Unset): When sent, exactly <issuer>/mcp.
        scope (TokenRequestScope | Unset):
    """

    grant_type: TokenRequestGrantType
    client_id: str
    code: str | Unset = UNSET
    redirect_uri: str | Unset = UNSET
    code_verifier: str | Unset = UNSET
    refresh_token: str | Unset = UNSET
    resource: str | Unset = UNSET
    scope: TokenRequestScope | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        grant_type = self.grant_type.value

        client_id = self.client_id

        code = self.code

        redirect_uri = self.redirect_uri

        code_verifier = self.code_verifier

        refresh_token = self.refresh_token

        resource = self.resource

        scope: str | Unset = UNSET
        if not isinstance(self.scope, Unset):
            scope = self.scope.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "grant_type": grant_type,
                "client_id": client_id,
            }
        )
        if code is not UNSET:
            field_dict["code"] = code
        if redirect_uri is not UNSET:
            field_dict["redirect_uri"] = redirect_uri
        if code_verifier is not UNSET:
            field_dict["code_verifier"] = code_verifier
        if refresh_token is not UNSET:
            field_dict["refresh_token"] = refresh_token
        if resource is not UNSET:
            field_dict["resource"] = resource
        if scope is not UNSET:
            field_dict["scope"] = scope

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        grant_type = TokenRequestGrantType(d.pop("grant_type"))

        client_id = d.pop("client_id")

        code = d.pop("code", UNSET)

        redirect_uri = d.pop("redirect_uri", UNSET)

        code_verifier = d.pop("code_verifier", UNSET)

        refresh_token = d.pop("refresh_token", UNSET)

        resource = d.pop("resource", UNSET)

        _scope = d.pop("scope", UNSET)
        scope: TokenRequestScope | Unset
        if isinstance(_scope, Unset):
            scope = UNSET
        else:
            scope = TokenRequestScope(_scope)

        token_request = cls(
            grant_type=grant_type,
            client_id=client_id,
            code=code,
            redirect_uri=redirect_uri,
            code_verifier=code_verifier,
            refresh_token=refresh_token,
            resource=resource,
            scope=scope,
        )

        token_request.additional_properties = d
        return token_request

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
