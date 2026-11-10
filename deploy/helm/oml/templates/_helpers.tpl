{{/*
The release's base name: the release name when it already contains the chart
name (a release named "oml" gives oml-server-lead and oml-server), otherwise
<release>-oml.
*/}}
{{- define "oml.fullname" -}}
{{- if contains .Chart.Name .Release.Name -}}
{{- .Release.Name | trunc 50 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 50 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "oml.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{/*
Every server pod, whatever its role: the Service and the NetworkPolicy select
by these, so a follower Deployment joins them without a selector change
(ADR-0033 d1).
*/}}
{{- define "oml.serverSelectorLabels" -}}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: server
{{- end -}}

{{/* oml-web's pods (ADR-0070 d4): their own component, outside the server selector. */}}
{{- define "oml.webSelectorLabels" -}}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: web
{{- end -}}

{{/* The Secret holding web.client_secret for the lead and oml-web (ADR-0070 d2). */}}
{{- define "oml.webClientSecret" -}}
{{- .Values.web.clientSecret.existingSecret | default (printf "%s-web-client" (include "oml.fullname" .)) -}}
{{- end -}}

{{/* oml-web's public origin: web.publicUrl, else https:// and the first Ingress host. */}}
{{- define "oml.webPublicUrl" -}}
{{- if .Values.web.publicUrl -}}
{{- .Values.web.publicUrl -}}
{{- else -}}
{{- printf "https://%s" (index .Values.ingress.hosts 0).host -}}
{{- end -}}
{{- end -}}

{{/* The lead's pods: the Deployment's immutable selector. */}}
{{- define "oml.leadSelectorLabels" -}}
{{ include "oml.serverSelectorLabels" . }}
oml.e8kor.github.io/role: lead
{{- end -}}

{{/* repository:tag, or repository:tag@digest when image.digest is set (ADR-0033 d9). */}}
{{- define "oml.image" -}}
{{- $ref := printf "%s:%s" .Values.image.repository (.Values.image.tag | default .Chart.AppVersion) -}}
{{- if .Values.image.digest -}}
{{- printf "%s@%s" $ref .Values.image.digest -}}
{{- else -}}
{{- $ref -}}
{{- end -}}
{{- end -}}

{{- define "oml.keyringClaim" -}}
{{- .Values.keyring.existingClaim | default (printf "%s-keyring" (include "oml.fullname" .)) -}}
{{- end -}}
