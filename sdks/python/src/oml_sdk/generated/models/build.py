from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.build_profile import BuildProfile

T = TypeVar("T", bound="Build")


@_attrs_define
class Build:
    """The build that answered (http-api.md "Health"). Times are UTC, YYYY-MM-DDTHH:MM:SSZ; a field nothing could supply is
    the string unknown.

        Attributes:
            version (str): The workspace version the binary was built at.
            commit (str): The full 40-hex commit
            commit_time (str): The commit's committer time
            build_time (str): When the build captured these fields; SOURCE_DATE_EPOCH fixes it when set.
            rustc (str): The compiler's rustc -V line.
            target (str): The target triple.
            profile (BuildProfile):
    """

    version: str
    commit: str
    commit_time: str
    build_time: str
    rustc: str
    target: str
    profile: BuildProfile

    def to_dict(self) -> dict[str, Any]:
        version = self.version

        commit = self.commit

        commit_time = self.commit_time

        build_time = self.build_time

        rustc = self.rustc

        target = self.target

        profile = self.profile.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "version": version,
                "commit": commit,
                "commit_time": commit_time,
                "build_time": build_time,
                "rustc": rustc,
                "target": target,
                "profile": profile,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        version = d.pop("version")

        commit = d.pop("commit")

        commit_time = d.pop("commit_time")

        build_time = d.pop("build_time")

        rustc = d.pop("rustc")

        target = d.pop("target")

        profile = BuildProfile(d.pop("profile"))

        build = cls(
            version=version,
            commit=commit,
            commit_time=commit_time,
            build_time=build_time,
            rustc=rustc,
            target=target,
            profile=profile,
        )

        return build
