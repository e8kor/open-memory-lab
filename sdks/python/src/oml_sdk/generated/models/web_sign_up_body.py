from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.web_sign_up_body_accept_terms import WebSignUpBodyAcceptTerms

T = TypeVar("T", bound="WebSignUpBody")


@_attrs_define
class WebSignUpBody:
    """
    Attributes:
        accept_terms (WebSignUpBodyAcceptTerms):
        terms_version (str):
    """

    accept_terms: WebSignUpBodyAcceptTerms
    terms_version: str

    def to_dict(self) -> dict[str, Any]:
        accept_terms = self.accept_terms.value

        terms_version = self.terms_version

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "accept_terms": accept_terms,
                "terms_version": terms_version,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        accept_terms = WebSignUpBodyAcceptTerms(d.pop("accept_terms"))

        terms_version = d.pop("terms_version")

        web_sign_up_body = cls(
            accept_terms=accept_terms,
            terms_version=terms_version,
        )

        return web_sign_up_body
