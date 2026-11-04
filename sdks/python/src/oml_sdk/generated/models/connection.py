from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.connection_client_kind import ConnectionClientKind
from ..models.connection_revoked_cause_type_1 import ConnectionRevokedCauseType1
from ..models.connection_revoked_cause_type_2_type_1 import (
    ConnectionRevokedCauseType2Type1,
)
from ..models.connection_revoked_cause_type_3_type_1 import (
    ConnectionRevokedCauseType3Type1,
)
from ..models.connection_state import ConnectionState
from ..types import UNSET, Unset

T = TypeVar("T", bound="Connection")


@_attrs_define
class Connection:
    """One of a connector key's connections, never a token or a digest of one. Whether it is live, the client's name and
    its opening and revocation times are designed (docs/api/http-api.md, Keys).

        Attributes:
            connection_id (str):
            client_id (str): The metadata-document URL or the registered client id.
            client_kind (ConnectionClientKind):
            redirect_uri (str):
            resource (str): <issuer>/mcp when the connection was created; its tokens work only while this is the current
                <issuer>/mcp.
            state (ConnectionState):
            revoked_cause (ConnectionRevokedCauseType1 | ConnectionRevokedCauseType2Type1 | ConnectionRevokedCauseType3Type1
                | None | Unset):
    """

    connection_id: str
    client_id: str
    client_kind: ConnectionClientKind
    redirect_uri: str
    resource: str
    state: ConnectionState
    revoked_cause: (
        ConnectionRevokedCauseType1
        | ConnectionRevokedCauseType2Type1
        | ConnectionRevokedCauseType3Type1
        | None
        | Unset
    ) = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        connection_id = self.connection_id

        client_id = self.client_id

        client_kind = self.client_kind.value

        redirect_uri = self.redirect_uri

        resource = self.resource

        state = self.state.value

        revoked_cause: None | str | Unset
        if isinstance(self.revoked_cause, Unset):
            revoked_cause = UNSET
        elif (
            isinstance(self.revoked_cause, ConnectionRevokedCauseType1)
            or isinstance(self.revoked_cause, ConnectionRevokedCauseType2Type1)
            or isinstance(self.revoked_cause, ConnectionRevokedCauseType3Type1)
        ):
            revoked_cause = self.revoked_cause.value
        else:
            revoked_cause = self.revoked_cause

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "connection_id": connection_id,
                "client_id": client_id,
                "client_kind": client_kind,
                "redirect_uri": redirect_uri,
                "resource": resource,
                "state": state,
            }
        )
        if revoked_cause is not UNSET:
            field_dict["revoked_cause"] = revoked_cause

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        connection_id = d.pop("connection_id")

        client_id = d.pop("client_id")

        client_kind = ConnectionClientKind(d.pop("client_kind"))

        redirect_uri = d.pop("redirect_uri")

        resource = d.pop("resource")

        state = ConnectionState(d.pop("state"))

        def _parse_revoked_cause(
            data: object,
        ) -> (
            ConnectionRevokedCauseType1
            | ConnectionRevokedCauseType2Type1
            | ConnectionRevokedCauseType3Type1
            | None
            | Unset
        ):
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                revoked_cause_type_1 = ConnectionRevokedCauseType1(data)

                return revoked_cause_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                revoked_cause_type_2_type_1 = ConnectionRevokedCauseType2Type1(data)

                return revoked_cause_type_2_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                revoked_cause_type_3_type_1 = ConnectionRevokedCauseType3Type1(data)

                return revoked_cause_type_3_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(
                ConnectionRevokedCauseType1
                | ConnectionRevokedCauseType2Type1
                | ConnectionRevokedCauseType3Type1
                | None
                | Unset,
                data,
            )

        revoked_cause = _parse_revoked_cause(d.pop("revoked_cause", UNSET))

        connection = cls(
            connection_id=connection_id,
            client_id=client_id,
            client_kind=client_kind,
            redirect_uri=redirect_uri,
            resource=resource,
            state=state,
            revoked_cause=revoked_cause,
        )

        connection.additional_properties = d
        return connection

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
