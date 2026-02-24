{{/*
Expand the name of the chart.
*/}}
{{- define "llm-proxy.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "llm-proxy.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "llm-proxy.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "llm-proxy.labels" -}}
helm.sh/chart: {{ include "llm-proxy.chart" . }}
{{ include "llm-proxy.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "llm-proxy.selectorLabels" -}}
app.kubernetes.io/name: {{ include "llm-proxy.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Service account name
*/}}
{{- define "llm-proxy.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "llm-proxy.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Resolve the image for the proxy server based on serverType
*/}}
{{- define "llm-proxy.image" -}}
{{- if eq .Values.serverType "rust" }}
{{- $tag := default .Chart.AppVersion .Values.image.rust.tag }}
{{- printf "%s:%s" .Values.image.rust.repository $tag }}
{{- else }}
{{- $tag := default .Chart.AppVersion .Values.image.python.tag }}
{{- printf "%s:%s" .Values.image.python.repository $tag }}
{{- end }}
{{- end }}

{{/*
Resolve image pull policy
*/}}
{{- define "llm-proxy.imagePullPolicy" -}}
{{- if eq .Values.serverType "rust" }}
{{- .Values.image.rust.pullPolicy }}
{{- else }}
{{- .Values.image.python.pullPolicy }}
{{- end }}
{{- end }}

{{/*
Resolve resource limits based on serverType
*/}}
{{- define "llm-proxy.resources" -}}
{{- if eq .Values.serverType "rust" }}
{{- toYaml .Values.resources.rust }}
{{- else }}
{{- toYaml .Values.resources.python }}
{{- end }}
{{- end }}

{{/*
Database secret name — which Secret holds DB_URL.
Uses existingSecret if provided, otherwise the chart-managed secret.
*/}}
{{- define "llm-proxy.databaseSecretName" -}}
{{- if .Values.database.existingSecret }}
{{- .Values.database.existingSecret }}
{{- else }}
{{- printf "%s-secret" (include "llm-proxy.fullname" .) }}
{{- end }}
{{- end }}

{{/*
Database secret key
*/}}
{{- define "llm-proxy.databaseSecretKey" -}}
{{- .Values.database.existingSecretKey | default "DB_URL" }}
{{- end }}

{{/*
Admin key secret name
*/}}
{{- define "llm-proxy.adminKeySecretName" -}}
{{- if .Values.adminKey.existingSecret }}
{{- .Values.adminKey.existingSecret }}
{{- else }}
{{- printf "%s-secret" (include "llm-proxy.fullname" .) }}
{{- end }}
{{- end }}

{{/*
Whether we need to create the chart-managed secret.
True when at least one of DB_URL or ADMIN_KEY is provided inline (not via existingSecret).
*/}}
{{- define "llm-proxy.createSecret" -}}
{{- if or (and (not .Values.database.existingSecret) .Values.database.url) (and (not .Values.adminKey.existingSecret) .Values.adminKey.value) }}
true
{{- end }}
{{- end }}
