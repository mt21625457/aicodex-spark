use chrono::{DateTime, Utc};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum GiteaPrReviewDbError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GiteaWebhookDelivery {
    pub delivery_id: String,
    pub event_type: String,
    pub created_at: DateTime<Utc>,
}

pub struct GiteaPrReviewRepository<'a> {
    pool: &'a PgPool,
}

#[derive(Debug, Clone)]
pub struct FeedbackAttempt {
    pub already_posted: bool,
}

impl<'a> GiteaPrReviewRepository<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Inserts delivery metadata if it does not exist yet.
    /// Returns `true` when inserted, `false` when it is a duplicate.
    pub async fn insert_delivery_if_new(
        &self,
        delivery_id: &str,
        event_type: &str,
    ) -> Result<bool, GiteaPrReviewDbError> {
        let result = sqlx::query(
            r#"
            INSERT INTO gitea_webhook_deliveries (delivery_id, event_type)
            VALUES ($1, $2)
            ON CONFLICT (delivery_id) DO NOTHING
            "#,
        )
        .bind(delivery_id)
        .bind(event_type)
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Register a feedback publish attempt.
    ///
    /// Returns `already_posted = true` when this review+state has already been
    /// published and should not be posted again.
    pub async fn begin_feedback_attempt(
        &self,
        review_id: Uuid,
        terminal_state: &str,
    ) -> Result<FeedbackAttempt, GiteaPrReviewDbError> {
        let row = sqlx::query_as::<_, (bool,)>(
            r#"
            INSERT INTO gitea_review_feedback (review_id, terminal_state, attempts, updated_at)
            VALUES ($1, $2, 1, NOW())
            ON CONFLICT (review_id, terminal_state)
            DO UPDATE SET
                attempts = gitea_review_feedback.attempts + 1,
                updated_at = NOW()
            RETURNING posted_at IS NOT NULL
            "#,
        )
        .bind(review_id)
        .bind(terminal_state)
        .fetch_one(self.pool)
        .await?;

        Ok(FeedbackAttempt {
            already_posted: row.0,
        })
    }

    pub async fn mark_feedback_posted(
        &self,
        review_id: Uuid,
        terminal_state: &str,
    ) -> Result<(), GiteaPrReviewDbError> {
        sqlx::query(
            r#"
            UPDATE gitea_review_feedback
            SET posted_at = NOW(), last_error = NULL, updated_at = NOW()
            WHERE review_id = $1 AND terminal_state = $2
            "#,
        )
        .bind(review_id)
        .bind(terminal_state)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    pub async fn mark_feedback_error(
        &self,
        review_id: Uuid,
        terminal_state: &str,
        error_message: &str,
    ) -> Result<(), GiteaPrReviewDbError> {
        sqlx::query(
            r#"
            UPDATE gitea_review_feedback
            SET last_error = $3, updated_at = NOW()
            WHERE review_id = $1 AND terminal_state = $2
            "#,
        )
        .bind(review_id)
        .bind(terminal_state)
        .bind(error_message)
        .execute(self.pool)
        .await?;

        Ok(())
    }
}
