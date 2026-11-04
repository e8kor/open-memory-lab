from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.token_response_scope import TokenResponseScope
from ..models.token_response_token_type import TokenResponseTokenType

T = TypeVar("T", bound="TokenResponse")


@_attrs_define
class TokenResponse:
    """
    Attributes:
        access_token (str): Opaque; the server stores only its SHA-256. A credential at /mcp alone, where it acts as
            exactly its connection's key (ADR-0031 d11).
        token_type (TokenResponseTokenType):
        expires_in (int): Seconds; the oauth.access_token_ttl setting.
        refresh_token (str): Opaque, single-use; rotated by every refresh.
        scope (TokenResponseScope):
    """

    access_token: str
    token_type: TokenResponseTokenType
    expires_in: int
    refresh_token: str
    scope: TokenResponseScope
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        access_token = self.access_token

        token_type = self.token_type.value

        expires_in = self.expires_in

        refresh_token = self.refresh_token

        scope = self.scope.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "access_token": access_token,
                "token_type": token_type,
                "expires_in": expires_in,
                "refresh_token": refresh_token,
                "scope": scope,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        access_token = d.pop("access_token")

        token_type = TokenResponseTokenType(d.pop("token_type"))

        expires_in = d.pop("expires_in")

        refresh_token = d.pop("refresh_token")

        scope = TokenResponseScope(d.pop("scope"))

        token_response = cls(
            access_token=access_token,
            token_type=token_type,
            expires_in=expires_in,
            refresh_token=refresh_token,
            scope=scope,
        )

        token_response.additional_properties = d
        return token_response

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
